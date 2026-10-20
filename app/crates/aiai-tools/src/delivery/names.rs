//! Nomes dos arquivos de entrega: modelo com `{projeto}`, `{episodio}`, `{corte}`, `{formato}`, `{data}`, `{versao}` (e
//! `{fonte}`, `{hora}`), nome seguro no Windows e versão automática (`v001`, `v002`...) que nunca sobrescreve uma entrega
//! anterior: a próxima versão é a maior que já existe na pasta (ou na fila) mais um, então apagar a v002 não reaproveita
//! o número.

use std::path::{Path, PathBuf};

/// Modelo de nome padrão.
pub const DEFAULT_TEMPLATE: &str = "{projeto}_{corte}_{formato}";

/// Tokens aceitos no modelo.
pub const TOKENS: [&str; 8] = ["{projeto}", "{episodio}", "{corte}", "{formato}", "{data}", "{hora}", "{fonte}", "{versao}"];

/// Valores que entram nos tokens (já em texto; a limpeza para o Windows é feita aqui).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NameCtx {
    pub project: String,
    pub episode: String,
    pub cut: String,
    pub format: String,
    /// `AAAA-MM-DD`.
    pub date: String,
    /// `HHMM`.
    pub time: String,
    /// Nome da fonte (edição, episódio ou corte).
    pub source: String,
}

/// Nome de arquivo seguro (sem separadores nem caracteres proibidos no Windows, sem ponto no fim, até 80 caracteres).
pub fn safe_name(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c }).collect();
    let t = cleaned.trim().trim_end_matches('.').trim();
    if t.is_empty() { "entrega".into() } else { t.chars().take(80).collect() }
}

/// Token desconhecido no modelo (`{foo}`) ou chave sem fechar vira erro com a lista dos aceitos.
pub fn validate_template(tpl: &str) -> Result<(), String> {
    if tpl.trim().is_empty() {
        return Err("o modelo de nome está vazio".into());
    }
    let mut rest = tpl;
    while let Some(i) = rest.find(['{', '}']) {
        if rest.as_bytes()[i] == b'}' {
            return Err(format!("'}}' sem '{{' no modelo \"{tpl}\""));
        }
        let Some(j) = rest[i..].find('}') else { return Err(format!("'{{' sem fechar no modelo \"{tpl}\"")) };
        let tok = &rest[i..=i + j];
        if !TOKENS.contains(&tok) {
            return Err(format!("token desconhecido {tok}; aceitos: {}", TOKENS.join(" ")));
        }
        rest = &rest[i + j + 1..];
    }
    Ok(())
}

fn value_of(tok: &str, c: &NameCtx) -> String {
    let v = match tok {
        "{projeto}" => &c.project,
        "{episodio}" => &c.episode,
        "{corte}" => &c.cut,
        "{formato}" => &c.format,
        "{data}" => &c.date,
        "{hora}" => &c.time,
        "{fonte}" => &c.source,
        _ => return String::new(),
    };
    if v.trim().is_empty() { String::new() } else { safe_name(v) }
}

/// Texto do modelo em duas partes: antes e depois de `{versao}` (cada uma já com os tokens trocados). Sem `{versao}` só
/// há a primeira.
fn render_parts(tpl: &str, c: &NameCtx) -> (String, Option<String>) {
    let render = |part: &str| {
        let mut out = String::new();
        let mut rest = part;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            match rest[i..].find('}') {
                Some(j) => {
                    out.push_str(&value_of(&rest[i..=i + j], c));
                    rest = &rest[i + j + 1..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    };
    match tpl.find("{versao}") {
        Some(i) => (render(&tpl[..i]), Some(render(&tpl[i + "{versao}".len()..]))),
        None => (render(tpl), None),
    }
}

/// Junta separadores repetidos que sobram de tokens vazios ("a__b" → "a_b") e apara as pontas.
fn tidy(s: &str) -> String {
    let mut out = String::new();
    let mut last: Option<char> = None;
    for ch in s.chars() {
        if matches!(ch, '_' | '-' | ' ') && last == Some(ch) {
            continue;
        }
        out.push(ch);
        last = Some(ch);
    }
    out.trim_matches(['_', '-', ' ', '.']).to_owned()
}

/// `v001` para a versão `n`.
pub fn version_label(n: u32) -> String {
    format!("v{n:03}")
}

/// Nome (sem extensão) do modelo com a versão dada. Sem `{versao}` no modelo o nome não leva versão.
pub fn render_template(tpl: &str, c: &NameCtx, version: Option<u32>) -> String {
    let (pre, post) = render_parts(tpl, c);
    let name = match post {
        Some(post) => format!("{pre}{}{post}", version_label(version.unwrap_or(1))),
        None => pre,
    };
    safe_name(&tidy(&name))
}

/// Maior versão já usada por um nome que começa com `pre` e continua com `v###` e `post` (antes da extensão ou do sufixo
/// de legenda `.pt.srt`).
fn max_version_in(names: impl Iterator<Item = String>, pre: &str, post: &str) -> u32 {
    let mut max = 0;
    for n in names {
        let Some(rest) = n.strip_prefix(pre) else { continue };
        let Some(rest) = rest.strip_prefix('v') else { continue };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.len() < 3 {
            continue;
        }
        let after = &rest[digits.len()..];
        // depois do número vem o resto do modelo e então a extensão: nada de letras coladas no número
        let ok = match after.strip_prefix(post) {
            Some(tail) => tail.is_empty() || tail.starts_with('.'),
            None => false,
        };
        if ok {
            if let Ok(v) = digits.parse::<u32>() {
                max = max.max(v);
            }
        }
    }
    max
}

/// Caminho de saída: `dir/<nome>.<ext>` ou, em sequência de imagens, `dir/<nome>/<nome>_%05d.<ext>`. Com `auto_version` (ou
/// `{versao}` no modelo) a versão é a próxima livre na pasta, contando também os caminhos `reserved` (itens da fila).
pub fn resolve_output(dir: &Path, tpl: &str, c: &NameCtx, ext: &str, auto_version: bool, sequence: bool, reserved: &[PathBuf]) -> PathBuf {
    let tpl_v: String = if tpl.contains("{versao}") || !auto_version { tpl.to_owned() } else { format!("{tpl}_{{versao}}") };
    let (pre, post) = render_parts(&tpl_v, c);
    let name = match post {
        None => safe_name(&tidy(&pre)),
        Some(post) => {
            // pasta e fila: nomes de arquivo, ou de pasta quando é sequência
            let mut names: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            for r in reserved {
                let rel = if sequence { r.parent().and_then(|p| p.file_name()) } else { r.file_name() };
                if let Some(n) = rel {
                    names.push(n.to_string_lossy().into_owned());
                }
            }
            let n = max_version_in(names.into_iter(), &pre, &post) + 1;
            safe_name(&tidy(&format!("{pre}{}{post}", version_label(n))))
        }
    };
    if sequence { dir.join(&name).join(format!("{name}_%05d.{ext}")) } else { dir.join(format!("{name}.{ext}")) }
}

/// Diferença do fuso local para o UTC, em minutos (Brasília = −180). Fora do Windows, 0.
pub fn local_offset_minutes() -> i64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime([u16; 8]);
        #[repr(C)]
        struct Tzi {
            bias: i32,
            standard_name: [u16; 32],
            standard_date: SystemTime,
            standard_bias: i32,
            daylight_name: [u16; 32],
            daylight_date: SystemTime,
            daylight_bias: i32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetTimeZoneInformation(tzi: *mut Tzi) -> u32;
        }
        // SAFETY: estrutura com o layout de TIME_ZONE_INFORMATION, preenchida pelo sistema.
        unsafe {
            let mut t: Tzi = std::mem::zeroed();
            let r = GetTimeZoneInformation(&mut t);
            let extra = match r {
                1 => t.standard_bias,
                2 => t.daylight_bias,
                _ => 0,
            };
            -((t.bias + extra) as i64)
        }
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Como [`date_time_of`], no horário local do PC (a data do nome do arquivo é a do dia de quem exporta, não a do UTC).
pub fn local_date_time_of(ms: u64) -> (String, String) {
    let shifted = (ms as i64 + local_offset_minutes() * 60_000).max(0) as u64;
    date_time_of(shifted)
}

/// Dia (UTC, `AAAA-MM-DD`) e hora (`HHMM`) de um instante em ms desde a época Unix.
pub fn date_time_of(ms: u64) -> (String, String) {
    let day = crate::production_history::day_of(ms);
    let secs = (ms / 1000) % 86_400;
    (crate::production_history::format_date(day), format!("{:02}{:02}", secs / 3600, (secs / 60) % 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> NameCtx {
        NameCtx { project: "Última Rosa".into(), episode: "Ep 1".into(), cut: "C003".into(), format: "YouTube 1080p".into(), date: "2026-10-20".into(), time: "1405".into(), source: "Edição final".into() }
    }

    #[test]
    fn tokens_are_replaced_and_names_are_windows_safe() {
        let n = render_template("{projeto}_{episodio}_{corte}_{formato}_{data}", &ctx(), None);
        assert_eq!(n, "Última Rosa_Ep 1_C003_YouTube 1080p_2026-10-20");
        let mut c = ctx();
        c.project = "a/b:c*".into();
        assert_eq!(render_template("{projeto}", &c, None), "a_b_c");
        // token vazio não deixa separador duplo nem sobra na ponta
        c.episode.clear();
        assert_eq!(render_template("{projeto}_{episodio}_{corte}", &c, None), "a_b_c_C003");
        assert_eq!(render_template("{episodio}_{corte}", &c, None), "C003");
        assert_eq!(render_template("{versao}", &c, Some(7)), "v007");
    }

    #[test]
    fn templates_are_validated() {
        assert!(validate_template("{projeto}_{versao}").is_ok());
        assert!(validate_template("{foo}").unwrap_err().contains("{foo}"));
        assert!(validate_template("{projeto").is_err());
        assert!(validate_template("a}b").is_err());
        assert!(validate_template("  ").is_err());
        assert!(validate_template("sem tokens").is_ok());
    }

    #[test]
    fn versions_count_up_and_never_reuse_a_deleted_number() {
        let dir = std::env::temp_dir().join(format!("aiai_names_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let c = ctx();
        let first = resolve_output(&dir, "{projeto}_{corte}", &c, "mp4", true, false, &[]);
        assert_eq!(first.file_name().unwrap().to_string_lossy(), "Última Rosa_C003_v001.mp4");
        std::fs::write(&first, b"x").unwrap();
        std::fs::write(dir.join("Última Rosa_C003_v002.mp4"), b"x").unwrap();
        std::fs::write(dir.join("Última Rosa_C003_v002.pt.srt"), b"x").unwrap();
        let next = resolve_output(&dir, "{projeto}_{corte}", &c, "mp4", true, false, &[]);
        assert_eq!(next.file_name().unwrap().to_string_lossy(), "Última Rosa_C003_v003.mp4");
        // a fila também conta: o item reservado ainda não existe em disco
        let reserved = vec![next.clone()];
        let after = resolve_output(&dir, "{projeto}_{corte}", &c, "mp4", true, false, &reserved);
        assert_eq!(after.file_name().unwrap().to_string_lossy(), "Última Rosa_C003_v004.mp4");
        // {versao} no meio do modelo
        std::fs::write(dir.join("a_v010_final.mp4"), b"x").unwrap();
        let mid = resolve_output(&dir, "a_{versao}_final", &c, "mp4", false, false, &[]);
        assert_eq!(mid.file_name().unwrap().to_string_lossy(), "a_v011_final.mp4");
        // outro modelo não interfere
        let other = resolve_output(&dir, "{projeto}_{formato}", &c, "mp4", true, false, &[]);
        assert!(other.file_name().unwrap().to_string_lossy().ends_with("_v001.mp4"));
        // sem versão automática o nome é o do modelo (sobrescreve: a interface avisa)
        let plain = resolve_output(&dir, "{corte}", &c, "mp4", false, false, &[]);
        assert_eq!(plain.file_name().unwrap().to_string_lossy(), "C003.mp4");
        // sequência de imagens: a versão está na pasta
        let seq = resolve_output(&dir, "{corte}", &c, "png", true, true, &[]);
        assert_eq!(seq, dir.join("C003_v001").join("C003_v001_%05d.png"));
        std::fs::create_dir_all(dir.join("C003_v001")).unwrap();
        let seq2 = resolve_output(&dir, "{corte}", &c, "png", true, true, &[]);
        assert_eq!(seq2, dir.join("C003_v002").join("C003_v002_%05d.png"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn local_time_is_a_sane_shift_of_utc() {
        assert!(local_offset_minutes().abs() <= 14 * 60);
        let ms = 1_760_000_000_000u64;
        let (_, utc_time) = date_time_of(ms);
        let (_, local_time) = local_date_time_of(ms);
        let mins = |t: &str| t[..2].parse::<i64>().unwrap() * 60 + t[2..].parse::<i64>().unwrap();
        assert_eq!((mins(&utc_time) + local_offset_minutes()).rem_euclid(1440), mins(&local_time));
    }

    #[test]
    fn dates_come_from_the_clock_value() {
        // 2026-10-20 14:05:30 UTC
        let day = crate::production_history::days_from_civil(2026, 10, 20) as u64;
        let ms = day * 86_400_000 + (14 * 3600 + 5 * 60 + 30) * 1000;
        assert_eq!(date_time_of(ms), ("2026-10-20".to_owned(), "1405".to_owned()));
    }
}
