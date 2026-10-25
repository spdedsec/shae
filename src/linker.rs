use crate::bundle::{BUNDLE_MAGIC, collect_bundle_opts};
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFormat {
    Elf64,
    Pe64,
    Auto,
}

impl LinkFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "elf" | "elf64" | "linux" => Some(LinkFormat::Elf64),
            "pe" | "pe64" | "exe" | "windows" => Some(LinkFormat::Pe64),
            "auto" => Some(LinkFormat::Auto),
            _ => None,
        }
    }
}

pub struct NativeLinker;

impl NativeLinker {
    pub fn link_to_elf(
        entry_file: &Path,
        output_file: &Path,
        strip_source: bool,
    ) -> Result<usize, String> {
        Self::link_with_format(entry_file, output_file, LinkFormat::Elf64, strip_source)
    }

    pub fn link_to_pe(
        entry_file: &Path,
        output_file: &Path,
        strip_source: bool,
    ) -> Result<usize, String> {
        Self::link_with_format(entry_file, output_file, LinkFormat::Pe64, strip_source)
    }

    pub fn link_with_format(
        entry_file: &Path,
        output_file: &Path,
        format: LinkFormat,
        strip_source: bool,
    ) -> Result<usize, String> {
        let actual_format = match format {
            LinkFormat::Auto => {
                if cfg!(windows) {
                    LinkFormat::Pe64
                } else {
                    LinkFormat::Elf64
                }
            }
            other => other,
        };

        let archive = collect_bundle_opts(entry_file, true, strip_source)
            .map_err(|e| format!("Failed to collect module bundle: {}", e))?;

        let payload = serde_json::to_vec(&archive)
            .map_err(|e| format!("Failed to serialize bundle payload: {}", e))?;
        let payload_len = payload.len() as u64;

        let current_exe = std::env::current_exe()
            .map_err(|e| format!("Failed to locate current executable stub: {}", e))?;
        let exe_path = if current_exe.to_string_lossy().contains("/deps/") {
            let candidate = current_exe
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("shae"));
            if let Some(c) = candidate {
                if c.exists() { c } else { current_exe }
            } else {
                current_exe
            }
        } else {
            current_exe
        };
        let exe_bytes = fs::read(&exe_path).map_err(|e| {
            format!(
                "Failed to read executable stub '{}': {}",
                exe_path.display(),
                e
            )
        })?;

        let mut out = fs::File::create(output_file)
            .map_err(|e| format!("Failed to create output binary: {}", e))?;

        match actual_format {
            LinkFormat::Elf64 => {
                // ELF 64-bit standalone executable
                out.write_all(&exe_bytes)
                    .map_err(|e| format!("Failed writing ELF stub: {}", e))?;
                out.write_all(&payload)
                    .map_err(|e| format!("Failed writing ELF payload: {}", e))?;
                out.write_all(&payload_len.to_be_bytes())
                    .map_err(|e| format!("Failed writing trailer length: {}", e))?;
                out.write_all(BUNDLE_MAGIC)
                    .map_err(|e| format!("Failed writing trailer magic: {}", e))?;
                out.flush()
                    .map_err(|e| format!("Failed flushing output: {}", e))?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = fs::metadata(output_file) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = fs::set_permissions(output_file, perms);
                    }
                }
            }
            LinkFormat::Pe64 => {
                // PE 64-bit executable (.exe)
                out.write_all(&exe_bytes)
                    .map_err(|e| format!("Failed writing PE stub: {}", e))?;
                out.write_all(&payload)
                    .map_err(|e| format!("Failed writing PE payload: {}", e))?;
                out.write_all(&payload_len.to_be_bytes())
                    .map_err(|e| format!("Failed writing trailer length: {}", e))?;
                out.write_all(BUNDLE_MAGIC)
                    .map_err(|e| format!("Failed writing trailer magic: {}", e))?;
                out.flush()
                    .map_err(|e| format!("Failed flushing output: {}", e))?;
            }
            LinkFormat::Auto => unreachable!(),
        }

        let total_size = fs::metadata(output_file)
            .map(|m| m.len() as usize)
            .unwrap_or(0);
        Ok(total_size)
    }

    /// Pure-Shae ELF 64-bit header builder verification helper
    pub fn build_elf_header(entry_addr: u64, ph_num: u16) -> Vec<u8> {
        let mut h = Vec::with_capacity(64);
        h.extend_from_slice(&[127, 69, 76, 70, 2, 1, 1, 0]); // \x7fELF, 64-bit, little-endian, EV_CURRENT
        h.extend_from_slice(&[0; 8]); // Padding
        h.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        h.extend_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        h.extend_from_slice(&1u32.to_le_bytes()); // EV_CURRENT
        h.extend_from_slice(&entry_addr.to_le_bytes());
        h.extend_from_slice(&64u64.to_le_bytes()); // ph_offset
        h.extend_from_slice(&0u64.to_le_bytes()); // sh_offset
        h.extend_from_slice(&0u32.to_le_bytes()); // flags
        h.extend_from_slice(&64u16.to_le_bytes()); // ehsize
        h.extend_from_slice(&56u16.to_le_bytes()); // phentsize
        h.extend_from_slice(&ph_num.to_le_bytes()); // phnum
        h.extend_from_slice(&64u16.to_le_bytes()); // shentsize
        h.extend_from_slice(&0u16.to_le_bytes()); // shnum
        h.extend_from_slice(&0u16.to_le_bytes()); // shstrndx
        h
    }

    /// Pure-Shae PE 64-bit DOS & COFF header builder verification helper
    pub fn build_pe_headers(num_sections: u16) -> Vec<u8> {
        let mut buf = Vec::with_capacity(512);
        // DOS Header (64 bytes)
        buf.extend_from_slice(b"MZ");
        buf.extend_from_slice(&[0u8; 58]);
        buf.extend_from_slice(&128u32.to_le_bytes()); // e_lfanew = 128 (0x80)

        // DOS Stub (64 bytes)
        let stub_msg = b"This program cannot be run in DOS mode.\r\r\n$";
        buf.extend_from_slice(stub_msg);
        while buf.len() < 128 {
            buf.push(0);
        }

        // PE Signature (4 bytes)
        buf.extend_from_slice(b"PE\0\0");
        // COFF Header (20 bytes)
        buf.extend_from_slice(&0x8664u16.to_le_bytes()); // AMD64
        buf.extend_from_slice(&num_sections.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
        buf.extend_from_slice(&0u32.to_le_bytes()); // SymbolTable
        buf.extend_from_slice(&0u32.to_le_bytes()); // NumSymbols
        buf.extend_from_slice(&240u16.to_le_bytes()); // SizeOfOptionalHeader
        buf.extend_from_slice(&0x0022u16.to_le_bytes()); // Characteristics: EXECUTABLE | LARGE_ADDRESS

        buf
    }
}
