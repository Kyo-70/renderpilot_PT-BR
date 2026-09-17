//! PE header parser and section locator with strict structural bounds.

use std::io::{self, Read, Seek, SeekFrom};

/// Maximum number of PE sections allowed.
pub const MAX_PE_SECTIONS: usize = 96;
/// Maximum bytes for section table.
pub const MAX_SECTION_TABLE_BYTES: usize = MAX_PE_SECTIONS * 40; // 3840 bytes
/// Structural limit for reading PE headers.
pub const MAX_HEADER_STRUCTURAL_BYTES: usize = 64 * 1024; // 64 KiB structural limit

/// AMD64 machine architecture code.
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
/// I386 machine architecture code.
pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014c;
/// Section contains initialized data.
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
/// Section is executable.
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
/// Section is readable.
pub const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
/// Section is writable.
pub const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;

/// Parsed PE section header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeSectionHeader {
    /// Section name (e.g. `.rdata`, `.text`).
    pub name: String,
    /// Virtual size of section in memory.
    pub virtual_size: u32,
    /// Relative virtual address of section in memory.
    pub virtual_address: u32,
    /// Size of raw data on disk.
    pub size_of_raw_data: u32,
    /// File pointer to raw data.
    pub pointer_to_raw_data: u32,
    /// Section flags and characteristics.
    pub characteristics: u32,
}

impl PeSectionHeader {
    /// Returns true if the section is read-only initialized data.
    #[must_use]
    pub fn is_readonly_initialized_data(&self) -> bool {
        (self.characteristics & IMAGE_SCN_MEM_READ != 0)
            && (self.characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA != 0)
            && (self.characteristics & IMAGE_SCN_MEM_EXECUTE == 0)
            && (self.characteristics & IMAGE_SCN_MEM_WRITE == 0)
    }

    /// Returns true if the section is writable initialized data.
    #[must_use]
    pub fn is_writable_initialized_data(&self) -> bool {
        (self.characteristics & IMAGE_SCN_MEM_READ != 0)
            && (self.characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA != 0)
            && (self.characteristics & IMAGE_SCN_MEM_EXECUTE == 0)
            && (self.characteristics & IMAGE_SCN_MEM_WRITE != 0)
    }
}

/// Parsed PE header metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeHeaderInfo {
    /// Machine architecture code.
    pub machine: u16,
    /// True if PE32+ (64-bit).
    pub is_64bit: bool,
    /// File offset to optional header.
    pub optional_header_offset: u64,
    /// Size of optional header in bytes.
    pub optional_header_size: u16,
    /// Number of RVA and sizes in data directories.
    pub number_of_rva_and_sizes: u32,
    /// Number of sections.
    pub number_of_sections: u16,
    /// List of parsed sections.
    pub sections: Vec<PeSectionHeader>,
}

/// Step-by-step PE header parser. Works with any `R: Read + Seek` (File, Cursor).
///
/// INVARIANT: `pe_offset` (`e_lfanew`) can reside at any valid position within the file.
/// Total structural header bytes are bounded by `MAX_HEADER_STRUCTURAL_BYTES`.
pub fn parse_pe_headers<R: Read + Seek>(reader: &mut R) -> io::Result<PeHeaderInfo> {
    let file_len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;

    if file_len < 64 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "File smaller than DOS header",
        ));
    }

    // 1. DOS Header (64 bytes)
    let mut dos_buf = [0u8; 64];
    reader.read_exact(&mut dos_buf)?;

    if dos_buf[0] != b'M' || dos_buf[1] != b'Z' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing MZ signature",
        ));
    }

    let pe_offset = u32::from_le_bytes(dos_buf[0x3C..0x40].try_into().unwrap()) as u64;
    if pe_offset.checked_add(24).is_none_or(|end| end > file_len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "PE header offset outside file bounds",
        ));
    }

    // 2. PE Signature & COFF Header (24 bytes)
    let mut coff_buf = [0u8; 24];
    reader.seek(SeekFrom::Start(pe_offset))?;
    reader.read_exact(&mut coff_buf)?;

    if &coff_buf[0..4] != b"PE\0\0" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing PE\\0\\0 signature",
        ));
    }

    let machine = u16::from_le_bytes(coff_buf[4..6].try_into().unwrap());
    let num_sections = u16::from_le_bytes(coff_buf[6..8].try_into().unwrap());
    let opt_hdr_size = u16::from_le_bytes(coff_buf[20..22].try_into().unwrap());

    // Structural check: OptionalHeader must have at least 2 bytes for magic
    if opt_hdr_size < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SizeOfOptionalHeader too small for magic",
        ));
    }

    if num_sections as usize > MAX_PE_SECTIONS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Exceeded MAX_PE_SECTIONS limit",
        ));
    }

    let opt_hdr_offset = pe_offset + 24;
    let sec_table_offset = opt_hdr_offset + opt_hdr_size as u64;
    let sec_table_bytes = (num_sections as u64) * 40;

    // Check structural byte budget (DOS 64 + COFF 24 + opt_hdr + sec_table)
    let total_structural_bytes = 64u64 + 24 + (opt_hdr_size as u64) + sec_table_bytes;
    if total_structural_bytes > MAX_HEADER_STRUCTURAL_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "PE structural headers exceed MAX_HEADER_STRUCTURAL_BYTES",
        ));
    }

    if sec_table_offset
        .checked_add(sec_table_bytes)
        .is_none_or(|end| end > file_len)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Section table outside file bounds",
        ));
    }

    // 3. Optional Header Magic
    let mut magic_buf = [0u8; 2];
    reader.seek(SeekFrom::Start(opt_hdr_offset))?;
    reader.read_exact(&mut magic_buf)?;
    let magic = u16::from_le_bytes(magic_buf);
    let is_64bit = match magic {
        0x020B => {
            // PE32+ must be >= 112 bytes for DataDirectories
            if opt_hdr_size < 112 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "PE32+ OptionalHeader too small",
                ));
            }
            true
        }
        0x010B => {
            // PE32 must be >= 96 bytes for DataDirectories
            if opt_hdr_size < 96 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "PE32 OptionalHeader too small",
                ));
            }
            false
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown Optional Header magic",
            ));
        }
    };

    // Read NumberOfRvaAndSizes
    let rva_sizes_offset = if is_64bit {
        opt_hdr_offset + 108
    } else {
        opt_hdr_offset + 92
    };
    let mut rva_buf = [0u8; 4];
    reader.seek(SeekFrom::Start(rva_sizes_offset))?;
    reader.read_exact(&mut rva_buf)?;
    let number_of_rva_and_sizes = u32::from_le_bytes(rva_buf);

    // 4. Section Table
    let mut sections = Vec::with_capacity(num_sections as usize);
    let mut sec_buf = vec![0u8; sec_table_bytes as usize];
    reader.seek(SeekFrom::Start(sec_table_offset))?;
    reader.read_exact(&mut sec_buf)?;

    for chunk in sec_buf.as_chunks::<40>().0 {
        let raw_name = &chunk[0..8];
        let name_len = raw_name.iter().position(|&b| b == 0).unwrap_or(8);
        let name = String::from_utf8_lossy(&raw_name[..name_len])
            .trim()
            .to_string();

        let virtual_size = u32::from_le_bytes(chunk[8..12].try_into().unwrap());
        let virtual_address = u32::from_le_bytes(chunk[12..16].try_into().unwrap());
        let size_of_raw_data = u32::from_le_bytes(chunk[16..20].try_into().unwrap());
        let pointer_to_raw_data = u32::from_le_bytes(chunk[20..24].try_into().unwrap());
        let characteristics = u32::from_le_bytes(chunk[36..40].try_into().unwrap());

        sections.push(PeSectionHeader {
            name,
            virtual_size,
            virtual_address,
            size_of_raw_data,
            pointer_to_raw_data,
            characteristics,
        });
    }

    Ok(PeHeaderInfo {
        machine,
        is_64bit,
        optional_header_offset: opt_hdr_offset,
        optional_header_size: opt_hdr_size,
        number_of_rva_and_sizes,
        number_of_sections: num_sections,
        sections,
    })
}

/// Target platform validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetPlatformError {
    /// Architecture is unsupported.
    UnsupportedArchitecture {
        /// PE machine field.
        machine: u16,
        /// Whether binary is 64-bit.
        is_64bit: bool,
    },
}

/// Mandatory architecture validator for PE files.
///
/// Supports Windows 64-bit AMD64 (PE32+, Machine 0x8664) and Windows 32-bit x86 (PE32, Machine 0x014C).
pub fn validate_pe_target_architecture(
    header: &PeHeaderInfo,
) -> Result<renderpilot_domain::Architecture, TargetPlatformError> {
    if header.machine == IMAGE_FILE_MACHINE_AMD64 && header.is_64bit {
        Ok(renderpilot_domain::Architecture::X64)
    } else if header.machine == IMAGE_FILE_MACHINE_I386 && !header.is_64bit {
        Ok(renderpilot_domain::Architecture::X86)
    } else {
        Err(TargetPlatformError::UnsupportedArchitecture {
            machine: header.machine,
            is_64bit: header.is_64bit,
        })
    }
}
