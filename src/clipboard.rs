use std::process::{Command, Stdio};

/// Copy text to the system clipboard using platform-specific commands.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
    } else if cfg!(target_os = "windows") {
        Command::new("clip")
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
    } else {
        // Linux: try xclip first, fall back to xsel
        Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
    };

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("clipboard command exited with {status}")),
        Err(e) => Err(format!("failed to run clipboard command: {e}")),
    }
}
