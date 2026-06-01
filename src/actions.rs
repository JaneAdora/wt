use suite_term::quote::shell_quote;

pub fn launch_command_for(
    cwd: &std::path::Path,
    resume_id: Option<&str>,
    dangerous: bool,
) -> String {
    let cwd_display = shell_quote(&cwd.to_string_lossy());
    let dangerous_flag = if dangerous {
        " --dangerously-skip-permissions"
    } else {
        ""
    };
    match resume_id {
        Some(id) => {
            let id_quoted = shell_quote(id);
            format!("cd {cwd_display} && claude --resume {id_quoted}{dangerous_flag}")
        }
        None => format!("cd {cwd_display} && claude{dangerous_flag}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn launch_command_no_resume() {
        let s = launch_command_for(Path::new("/home/jane/projects/example-project"), None, false);
        assert_eq!(s, "cd /home/jane/projects/example-project && claude");
    }

    #[test]
    fn launch_command_with_resume() {
        let s = launch_command_for(
            Path::new("/home/jane/projects/example-project"),
            Some("abc-123"),
            false,
        );
        assert_eq!(s, "cd /home/jane/projects/example-project && claude --resume abc-123");
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

    #[test]
    fn launch_command_quotes_path_with_spaces() {
        let s = launch_command_for(Path::new("/home/jane/My Repo"), Some("id1"), false);
        assert_eq!(s, "cd '/home/jane/My Repo' && claude --resume id1");
    }

    #[test]
    fn launch_command_quotes_resume_id_with_metachar() {
        let s = launch_command_for(Path::new("/p"), Some("a;b"), false);
        assert_eq!(s, "cd /p && claude --resume 'a;b'");
    }
}
