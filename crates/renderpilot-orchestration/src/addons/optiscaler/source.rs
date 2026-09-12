//! Official upstream source policy for OptiScaler release archives.
//!
//! RenderPilot distributes only its metadata manifest. Package bytes are
//! fetched by the client from the author's GitHub Releases repository. The
//! locator is transport metadata, while the manifest's digest, exact size and
//! member map form the immutable release identity.

use reqwest::Url;

use crate::{ServiceError, failed};

use super::types::{OptiScalerRelease, OptiScalerReleaseProvider, OptiScalerReleaseSource};

const OFFICIAL_REPOSITORY: &str = "optiscaler/OptiScaler";
const GITHUB_HOST: &str = "github.com";
const GITHUB_RELEASE_ASSET_HOST: &str = "release-assets.githubusercontent.com";
const OPTIPATCHER_REPOSITORY: &str = "optiscaler/OptiPatcher";
const GITHUB_BASE: &str = "https://github.com/";

pub(super) fn validate_release_source(release: &OptiScalerRelease) -> Result<(), ServiceError> {
    let source = &release.source;
    if source.provider != OptiScalerReleaseProvider::GithubRelease
        || source.repository != OFFICIAL_REPOSITORY
        || !safe_path_segment(&source.tag, 128)
        || !safe_path_segment(&source.asset, 255)
        || !source.asset.to_ascii_lowercase().ends_with(".7z")
        || source.tag != release.id
    {
        return Err(failed(format!(
            "OptiScaler release {} does not use an official pinned GitHub Release source",
            release.id
        )));
    }
    release_download_url(release).map(|_| ())
}

pub(super) fn validate_module_artifact_source(
    module_id: &str,
    source: &OptiScalerReleaseSource,
) -> Result<(), ServiceError> {
    let expected_repository = match module_id {
        "optipatcher" => OPTIPATCHER_REPOSITORY,
        _ => {
            return Err(failed(format!(
                "OptiScaler module {module_id} has no approved independent upstream source"
            )));
        }
    };
    if source.provider != OptiScalerReleaseProvider::GithubRelease
        || source.repository != expected_repository
        || !safe_path_segment(&source.tag, 128)
        || !safe_path_segment(&source.asset, 255)
        || !immutable_release_tag(&source.tag)
    {
        return Err(failed(format!(
            "OptiScaler module {module_id} does not use its approved official immutable GitHub Release source"
        )));
    }
    download_url_from_source(source).map(|_| ())
}

/// Builds the official browser-download URL from typed source components.
/// Arbitrary URLs are intentionally not accepted from the manifest.
pub(super) fn release_download_url(release: &OptiScalerRelease) -> Result<Url, ServiceError> {
    download_url_from_source(&release.source)
}

pub(super) fn download_url_from_source(
    source: &OptiScalerReleaseSource,
) -> Result<Url, ServiceError> {
    let (owner, repository) = source.repository.split_once('/').ok_or_else(|| {
        failed("OptiScaler GitHub source repository must contain one owner/repository separator")
    })?;
    if !safe_path_segment(owner, 128)
        || !safe_path_segment(repository, 128)
        || repository.contains('/')
    {
        return Err(failed("invalid OptiScaler GitHub source repository"));
    }
    let mut url = Url::parse(GITHUB_BASE)
        .map_err(|error| failed(format!("invalid built-in OptiScaler source URL: {error}")))?;
    url.path_segments_mut()
        .map_err(|()| failed("built-in OptiScaler source URL cannot accept path segments"))?
        .pop_if_empty()
        .push(owner)
        .push(repository)
        .push("releases")
        .push("download")
        .push(&source.tag)
        .push(&source.asset);
    Ok(url)
}

/// Fail-closed redirect policy evaluated before each network request.
pub(super) fn is_allowed_download_url(expected: &Url, candidate: &Url) -> bool {
    if candidate == expected {
        return clean_https_authority(candidate, GITHUB_HOST);
    }
    clean_https_authority(candidate, GITHUB_RELEASE_ASSET_HOST)
        && candidate
            .path()
            .starts_with("/github-production-release-asset/")
}

/// Verifies the complete observed chain as a second provenance check after the
/// pre-request redirect guard.
pub(super) fn validate_download_chain(expected: &Url, chain: &[Url]) -> Result<(), ServiceError> {
    if chain.first() != Some(expected)
        || chain
            .iter()
            .any(|candidate| !is_allowed_download_url(expected, candidate))
    {
        return Err(failed(format!(
            "OptiScaler archive left the official GitHub Release source chain: {}",
            chain.last().map_or(expected.as_str(), Url::as_str)
        )));
    }
    Ok(())
}

fn clean_https_authority(url: &Url, host: &str) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
}

fn safe_path_segment(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | '?' | '#'))
}

fn immutable_release_tag(value: &str) -> bool {
    value.strip_prefix('v').is_some_and(|version| {
        !version.is_empty()
            && version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::optiscaler::types::{
        OptiScalerRelease, OptiScalerReleaseProvider, OptiScalerReleaseSource,
    };

    fn release() -> OptiScalerRelease {
        OptiScalerRelease {
            id: "v0.9.3".to_owned(),
            source: OptiScalerReleaseSource {
                provider: OptiScalerReleaseProvider::GithubRelease,
                repository: OFFICIAL_REPOSITORY.to_owned(),
                tag: "v0.9.3".to_owned(),
                asset: "Optiscaler_0.9.3-final.20260618.7z".to_owned(),
            },
            archive_sha256: "0".repeat(64),
            archive_size: 1,
            config_schema: 1,
            members: Vec::new(),
        }
    }

    #[test]
    fn derives_the_exact_official_release_asset_url() {
        let release = release();
        validate_release_source(&release).expect("official source");
        assert_eq!(
            release_download_url(&release).expect("url").as_str(),
            "https://github.com/optiscaler/OptiScaler/releases/download/v0.9.3/Optiscaler_0.9.3-final.20260618.7z"
        );
    }

    #[test]
    fn rejects_other_repositories_and_stable_tag_aliases() {
        let mut other_repo = release();
        other_repo.source.repository = "attacker/OptiScaler".to_owned();
        assert!(validate_release_source(&other_repo).is_err());

        let mut rolling_stable = release();
        rolling_stable.source.tag = "latest".to_owned();
        assert!(validate_release_source(&rolling_stable).is_err());
    }

    #[test]
    fn accepts_only_the_expected_start_and_github_asset_cdn_hops() {
        let expected = release_download_url(&release()).expect("url");
        let asset = Url::parse(
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/abc?sp=r",
        )
        .expect("asset");
        validate_download_chain(&expected, &[expected.clone(), asset]).expect("official chain");

        let evil = Url::parse("https://example.com/archive.7z").expect("evil");
        assert!(validate_download_chain(&expected, &[expected.clone(), evil]).is_err());
        let user_info = Url::parse(
            "https://user@release-assets.githubusercontent.com/github-production-release-asset/1/abc",
        )
        .expect("userinfo");
        assert!(validate_download_chain(&expected, &[expected.clone(), user_info]).is_err());
    }

    #[test]
    fn renderpilot_cdn_is_not_an_archive_source() {
        let expected = release_download_url(&release()).expect("url");
        let cdn = Url::parse(
            "https://pub-48612a35034d40f88f42b4181547925a.r2.dev/addons/optiscaler/archive.7z",
        )
        .expect("cdn");
        assert!(validate_download_chain(&expected, &[expected.clone(), cdn]).is_err());
    }

    #[test]
    fn optipatcher_uses_only_its_official_upstream_repository() {
        let source = OptiScalerReleaseSource {
            provider: OptiScalerReleaseProvider::GithubRelease,
            repository: OPTIPATCHER_REPOSITORY.to_owned(),
            tag: "v0.41".to_owned(),
            asset: "OptiPatcher_v0.41.asi".to_owned(),
        };
        validate_module_artifact_source("optipatcher", &source).expect("official source");
        assert_eq!(
            download_url_from_source(&source).expect("url").as_str(),
            "https://github.com/optiscaler/OptiPatcher/releases/download/v0.41/OptiPatcher_v0.41.asi"
        );

        let mut attacker = source.clone();
        attacker.repository = "attacker/OptiPatcher".to_owned();
        assert!(validate_module_artifact_source("optipatcher", &attacker).is_err());

        let mut mutable_alias = source;
        mutable_alias.tag = "rolling".to_owned();
        assert!(validate_module_artifact_source("optipatcher", &mutable_alias).is_err());
    }
}
