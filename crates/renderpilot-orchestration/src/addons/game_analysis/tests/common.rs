use std::path::Path;

use renderpilot_domain::{
    Architecture, ExeGraphicsInfo, GameIdentity, GameInstallation, GameRuntime, GraphicsApi,
    Launcher, PathRef, Platform,
};

use crate::game_executable::{ExeSource, ResolvedExecutable};

pub fn path(value: &str) -> PathRef {
    PathRef::new(value).expect("valid path")
}

pub fn resolved(path_str: &str, apis: &[GraphicsApi]) -> ResolvedExecutable {
    let file_name = Path::new(path_str)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    ResolvedExecutable {
        path: path(path_str),
        file_name,
        graphics: ExeGraphicsInfo::new(apis.to_vec(), Some(Architecture::X64)),
        source: ExeSource::Auto,
    }
}

pub fn resolved_with_arch(path_str: &str, arch: Architecture) -> ResolvedExecutable {
    let file_name = Path::new(path_str)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    ResolvedExecutable {
        path: path(path_str),
        file_name,
        graphics: ExeGraphicsInfo::new(Vec::new(), Some(arch)),
        source: ExeSource::Auto,
    }
}

pub fn install_in(dir: &Path, candidate: &str) -> GameInstallation {
    let identity = GameIdentity::new(
        renderpilot_domain::GameId::new("steam:1091500").expect("id"),
        "My Game",
        Launcher::Steam,
    )
    .expect("identity")
    .with_external_id("1091500")
    .expect("external id");

    GameInstallation::new(
        identity,
        Platform::Windows,
        GameRuntime::NativeWindows,
        path(&dir.to_string_lossy()),
    )
    .with_executable_candidate(path(candidate))
}

pub fn create_minimal_valid_pe(arch: Architecture) -> Vec<u8> {
    let mut pe = vec![0u8; 512];
    pe[0] = b'M';
    pe[1] = b'Z';
    let pe_offset: u32 = 0x80;
    pe[0x3c..0x40].copy_from_slice(&pe_offset.to_le_bytes());

    let off = pe_offset as usize;
    pe[off..off + 4].copy_from_slice(b"PE\0\0");

    let is_64 = arch == Architecture::X64;
    let machine: u16 = if is_64 { 0x8664 } else { 0x014c };
    pe[off + 4..off + 6].copy_from_slice(&machine.to_le_bytes());
    let num_sections: u16 = 0;
    pe[off + 6..off + 8].copy_from_slice(&num_sections.to_le_bytes());
    let opt_hdr_size: u16 = 0xF0;
    pe[off + 20..off + 22].copy_from_slice(&opt_hdr_size.to_le_bytes());

    let opt_off = off + 24;
    let magic: u16 = if is_64 { 0x020B } else { 0x010B };
    pe[opt_off..opt_off + 2].copy_from_slice(&magic.to_le_bytes());

    let rva_sizes_offset = if is_64 { opt_off + 108 } else { opt_off + 92 };
    pe[rva_sizes_offset..rva_sizes_offset + 4].copy_from_slice(&16u32.to_le_bytes());

    pe
}

pub fn write_minimal_pe(path: &Path, arch: Architecture) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, create_minimal_valid_pe(arch)).unwrap();
}

pub fn write_synthetic_pe(path: &Path, sections: &[(&str, u32, &[u8])]) {
    write_synthetic_pe_advanced(path, sections, None);
}

pub fn write_synthetic_pe_advanced(
    path: &Path,
    sections: &[(&str, u32, &[u8])],
    truncate_to: Option<usize>,
) {
    let pe_offset: usize = 0x80;
    let optional_header_size: usize = 0xF0;
    let coff_offset = pe_offset + 4;
    let optional_header_offset = coff_offset + 20;
    let optional_header_end = optional_header_offset + optional_header_size;
    let section_table_offset = optional_header_end;
    let section_count = sections.len() as u16;
    let section_table_size = sections.len() * 40;
    let headers_end = section_table_offset + section_table_size;

    let mut current_file_offset = (headers_end + 0x1FF) & !0x1FF;
    if current_file_offset < 0x200 {
        current_file_offset = 0x200;
    }

    let mut total_size = current_file_offset;
    for (_name, _chars, data) in sections {
        total_size += data.len();
        total_size = (total_size + 0x1FF) & !0x1FF;
    }

    let mut bytes = vec![0u8; total_size];
    bytes[0..2].copy_from_slice(b"MZ");
    bytes[0x3C..0x40].copy_from_slice(&(pe_offset as u32).to_le_bytes());
    bytes[pe_offset..pe_offset + 4].copy_from_slice(b"PE\0\0");
    bytes[coff_offset..coff_offset + 2].copy_from_slice(&0x8664u16.to_le_bytes()); // AMD64
    bytes[coff_offset + 2..coff_offset + 4].copy_from_slice(&section_count.to_le_bytes());
    bytes[coff_offset + 16..coff_offset + 18]
        .copy_from_slice(&(optional_header_size as u16).to_le_bytes());
    bytes[optional_header_offset..optional_header_offset + 2]
        .copy_from_slice(&0x020Bu16.to_le_bytes()); // PE32+
    let rva_sizes_offset = optional_header_offset + 108;
    bytes[rva_sizes_offset..rva_sizes_offset + 4].copy_from_slice(&16u32.to_le_bytes());

    let mut cur_raw_ptr = current_file_offset;
    let mut cur_rva = 0x1000u32;
    for (i, (name, chars, data)) in sections.iter().enumerate() {
        let sec_entry = section_table_offset + i * 40;
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(8);
        bytes[sec_entry..sec_entry + len].copy_from_slice(&name_bytes[..len]);

        let raw_size = data.len() as u32;
        let virt_size = raw_size.max(0x1000);
        bytes[sec_entry + 8..sec_entry + 12].copy_from_slice(&virt_size.to_le_bytes());
        bytes[sec_entry + 12..sec_entry + 16].copy_from_slice(&cur_rva.to_le_bytes());
        bytes[sec_entry + 16..sec_entry + 20].copy_from_slice(&raw_size.to_le_bytes());
        bytes[sec_entry + 20..sec_entry + 24].copy_from_slice(&(cur_raw_ptr as u32).to_le_bytes());
        bytes[sec_entry + 36..sec_entry + 40].copy_from_slice(&chars.to_le_bytes());

        bytes[cur_raw_ptr..cur_raw_ptr + data.len()].copy_from_slice(data);

        cur_raw_ptr += (data.len() + 0x1FF) & !0x1FF;
        cur_rva += (virt_size + 0xFFF) & !0xFFF;
    }

    if let Some(limit) = truncate_to {
        bytes.truncate(limit);
    }

    std::fs::write(path, bytes).expect("write synthetic PE file");
}

pub fn write_synthetic_pe_with_pdb(path: &Path, pdb_name: &str) {
    let pe_offset: usize = 0x80;
    let optional_header_size: usize = 0xF0;
    let coff_offset = pe_offset + 4;
    let optional_header_offset = coff_offset + 20;
    let optional_header_end = optional_header_offset + optional_header_size;
    let section_table_offset = optional_header_end;
    let section_count = 1u16;
    let section_table_size = 40;
    let headers_end = section_table_offset + section_table_size;

    let mut current_file_offset = (headers_end + 0x1FF) & !0x1FF;
    if current_file_offset < 0x200 {
        current_file_offset = 0x200;
    }

    let mut cv_record = Vec::new();
    cv_record.extend_from_slice(b"RSDS");
    cv_record.extend_from_slice(&[0u8; 16]); // GUID
    cv_record.extend_from_slice(&1u32.to_le_bytes()); // Age
    cv_record.extend_from_slice(pdb_name.as_bytes());
    cv_record.push(0); // NUL terminator

    let debug_entry_size = 28;
    let rdata_rva = 0x1000u32;
    let cv_rva = rdata_rva + debug_entry_size as u32;
    let cv_raw_ptr = (current_file_offset + debug_entry_size) as u32;

    let mut rdata_section = vec![0u8; debug_entry_size];
    rdata_section[12..16].copy_from_slice(&2u32.to_le_bytes()); // Type = IMAGE_DEBUG_TYPE_CODEVIEW (2)
    rdata_section[16..20].copy_from_slice(&(cv_record.len() as u32).to_le_bytes()); // SizeOfData
    rdata_section[20..24].copy_from_slice(&cv_rva.to_le_bytes()); // AddressOfRawData
    rdata_section[24..28].copy_from_slice(&cv_raw_ptr.to_le_bytes()); // PointerToRawData
    rdata_section.extend_from_slice(&cv_record);

    let total_size = (current_file_offset + rdata_section.len() + 0x1FF) & !0x1FF;
    let mut bytes = vec![0u8; total_size];
    bytes[0..2].copy_from_slice(b"MZ");
    bytes[0x3C..0x40].copy_from_slice(&(pe_offset as u32).to_le_bytes());
    bytes[pe_offset..pe_offset + 4].copy_from_slice(b"PE\0\0");
    bytes[coff_offset..coff_offset + 2].copy_from_slice(&0x8664u16.to_le_bytes()); // AMD64
    bytes[coff_offset + 2..coff_offset + 4].copy_from_slice(&section_count.to_le_bytes());
    bytes[coff_offset + 16..coff_offset + 18]
        .copy_from_slice(&(optional_header_size as u16).to_le_bytes());
    bytes[optional_header_offset..optional_header_offset + 2]
        .copy_from_slice(&0x020Bu16.to_le_bytes()); // PE32+
    let rva_sizes_offset = optional_header_offset + 108;
    bytes[rva_sizes_offset..rva_sizes_offset + 4].copy_from_slice(&16u32.to_le_bytes());

    // Set Data Directory[6] (IMAGE_DIRECTORY_ENTRY_DEBUG)
    let data_dirs_offset = optional_header_offset + 112;
    let debug_dir_entry_offset = data_dirs_offset + (6 * 8);
    bytes[debug_dir_entry_offset..debug_dir_entry_offset + 4]
        .copy_from_slice(&rdata_rva.to_le_bytes());
    bytes[debug_dir_entry_offset + 4..debug_dir_entry_offset + 8]
        .copy_from_slice(&(debug_entry_size as u32).to_le_bytes());

    // Write .rdata section header
    let sec_entry = section_table_offset;
    bytes[sec_entry..sec_entry + 6].copy_from_slice(b".rdata");
    let raw_size = rdata_section.len() as u32;
    let virt_size = raw_size.max(0x1000);
    bytes[sec_entry + 8..sec_entry + 12].copy_from_slice(&virt_size.to_le_bytes());
    bytes[sec_entry + 12..sec_entry + 16].copy_from_slice(&rdata_rva.to_le_bytes());
    bytes[sec_entry + 16..sec_entry + 20].copy_from_slice(&raw_size.to_le_bytes());
    bytes[sec_entry + 20..sec_entry + 24]
        .copy_from_slice(&(current_file_offset as u32).to_le_bytes());
    bytes[sec_entry + 36..sec_entry + 40].copy_from_slice(&0x4000_0040u32.to_le_bytes());

    // Write .rdata content
    bytes[current_file_offset..current_file_offset + rdata_section.len()]
        .copy_from_slice(&rdata_section);

    std::fs::write(path, bytes).expect("write synthetic PE file with PDB");
}
