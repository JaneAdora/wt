use anyhow::Result;
use base64::Engine;

/// Encode a string as an OSC 52 escape sequence: `\x1b]52;c;<base64>\x07`
pub fn osc52_encode(s: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(s);
    format!("\x1b]52;c;{b64}\x07")
}

pub fn launch_command_for(
    cwd: &std::path::Path,
    resume_id: Option<&str>,
    dangerous: bool,
) -> String {
    let cwd_display = cwd.to_string_lossy();
    let dangerous_flag = if dangerous {
        " --dangerously-skip-permissions"
    } else {
        ""
    };
    match resume_id {
        Some(id) => format!("cd {cwd_display} && claude --resume {id}{dangerous_flag}"),
        None => format!("cd {cwd_display} && claude{dangerous_flag}"),
    }
}

/// Write the OSC 52 sequence to stdout. The terminal honors or drops it.
pub fn copy_to_clipboard(s: &str) -> Result<()> {
    use std::io::Write;
    let seq = osc52_encode(s);
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(seq.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn osc52_encodes_with_correct_envelope() {
        let out = osc52_encode("hello");
        assert!(out.starts_with("\x1b]52;c;"));
        assert!(out.ends_with('\x07'));
        let body = &out[7..out.len() - 1];
        let decoded = base64::engine::general_purpose::STANDARD.decode(body).unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn launch_command_no_resume() {
        let s = launch_command_for(Path::new("/home/jane/projects/thelma"), None, false);
        assert_eq!(s, "cd /home/jane/projects/thelma && claude");
    }

    #[test]
    fn launch_command_with_resume() {
        let s = launch_command_for(
            Path::new("/home/jane/projects/thelma"),
            Some("abc-123"),
            false,
        );
        assert_eq!(s, "cd /home/jane/projects/thelma && claude --resume abc-123");
    }

    #[test]
    fn launch_command_dangerous_no_resume() {
        let s = launch_command_for(Path::new("/p/x"), None, true);
        assert_eq!(s, "cd /p/x && claude --dangerously-skip-permissions");
    }

    #[test]
    fn launch_command_dangerous_with_resume() {
        let s = launch_command_for(Path::new("/p/x"), Some("zzz-1"), true);
        assert_eq!(
            s,
            "cd /p/x && claude --resume zzz-1 --dangerously-skip-permissions"
        );
    }
}
