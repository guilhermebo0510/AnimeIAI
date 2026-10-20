//! Embute o ícone (`../aiai-studio/assets/brand/aiai.ico`) e as informações de versão no `aiai-mcp.exe` (ferramenta de
//! console): o Explorer e o atalho mostram a mesma marca do estúdio. Ver `aiai-studio/build.rs`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../aiai-studio/assets/brand/aiai.ico");
    #[cfg(windows)]
    {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let out = std::env::var("OUT_DIR").expect("OUT_DIR");
        let ico = std::path::Path::new(&manifest).join("..").join("aiai-studio").join("assets").join("brand").join("aiai.ico");
        assert!(ico.is_file(), "ícone ausente: {} (rode tools/brand/make_icons.py)", ico.display());
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());
        let rc = rc_text(&ico, &version, "AnimeIAI MCP", "aiai-mcp.exe", "servidor MCP e linha de comando do AnimeIAI");
        let rc_path = std::path::Path::new(&out).join("aiai-mcp.rc");
        std::fs::write(&rc_path, rc).expect("gravar .rc");
        embed_resource::compile(&rc_path, embed_resource::NONE).manifest_required().expect("rc.exe (Windows SDK) para embutir o ícone");
    }
}

/// Texto do .rc em UTF-16 LE com BOM (o `rc.exe` lê assim sem depender de code page).
#[cfg(windows)]
fn rc_text(ico: &std::path::Path, version: &str, product: &str, file: &str, description: &str) -> Vec<u8> {
    let mut n = version.split('.').map(|p| p.parse::<u16>().unwrap_or(0)).chain(std::iter::repeat(0));
    let (a, b, c) = (n.next().unwrap(), n.next().unwrap(), n.next().unwrap());
    let ico = ico.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
    let text = format!(
        "1 ICON \"{ico}\"\n\
         1 VERSIONINFO\n\
         FILEVERSION {a},{b},{c},0\n\
         PRODUCTVERSION {a},{b},{c},0\n\
         FILEFLAGSMASK 0x3fL\n\
         FILEFLAGS 0x0L\n\
         FILEOS 0x40004L\n\
         FILETYPE 0x1L\n\
         FILESUBTYPE 0x0L\n\
         BEGIN\n\
           BLOCK \"StringFileInfo\"\n\
           BEGIN\n\
             BLOCK \"041604B0\"\n\
             BEGIN\n\
               VALUE \"CompanyName\", \"AnimeIAI\"\n\
               VALUE \"FileDescription\", \"{product} - {description}\"\n\
               VALUE \"FileVersion\", \"{version}\"\n\
               VALUE \"InternalName\", \"{file}\"\n\
               VALUE \"LegalCopyright\", \"\u{a9} 2026 AnimeIAI\"\n\
               VALUE \"OriginalFilename\", \"{file}\"\n\
               VALUE \"ProductName\", \"{product}\"\n\
               VALUE \"ProductVersion\", \"{version}\"\n\
             END\n\
           END\n\
           BLOCK \"VarFileInfo\"\n\
           BEGIN\n\
             VALUE \"Translation\", 0x0416, 1200\n\
           END\n\
         END\n"
    );
    let mut bytes = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    bytes
}
