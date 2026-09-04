//! Cross-platform process management utilities.

use std::process::Command;

/// Execute a shell script in a cross-platform way.
///
/// # Unix
/// Uses `sh -c <script>`
///
/// # Windows  
/// Uses `powershell.exe -Command <script>`
///
/// # Errors
/// Returns IoError if the command cannot be executed.
pub fn execute_script(script: &str) -> std::io::Result<std::process::Output> {
    #[cfg(unix)]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd.output()
    }

    #[cfg(windows)]
    {
        // Try PowerShell first, fall back to cmd.exe
        let mut cmd = Command::new("powershell.exe");
        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(script);

        match cmd.output() {
            Ok(output) => Ok(output),
            Err(_) => {
                // Fallback to cmd.exe
                let mut cmd = Command::new("cmd.exe");
                cmd.arg("/C").arg(script);
                cmd.output()
            }
        }
    }
}

/// Execute a command with arguments (cross-platform).
///
/// # Examples
/// ```rust,no_run
/// use rg_core::platform::process::execute_command;
/// let output = execute_command("git", &["status"]).unwrap();
/// ```
pub fn execute_command(program: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.output()
}

/// Terminate a process (cross-platform).
///
/// # Unix
/// Uses `kill -9 <pid>`
///
/// # Windows
/// Uses `taskkill /PID <pid> /F`
pub fn terminate_process(pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output()?;
        Ok(())
    }

    #[cfg(windows)]
    {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()?;
        Ok(())
    }
}

/// Check if a process is running (cross-platform).
///
/// # Unix
/// Uses `kill -0 <pid>` to check if process exists.
///
/// # Windows
/// Uses `tasklist /FI "PID eq <pid>"` to check.
pub fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains(&pid.to_string())
            }
            Err(_) => false,
        }
    }
}

/// Variables Windows PowerShell needs in order to boot at all.
///
/// Job scripts run with a cleared environment (`Command::env_clear`) for
/// isolation, but PowerShell fails to initialise without these and aborts with
/// crypto/NTE error `0x8009001d` — printing its banner instead of running the
/// script. Only this fixed whitelist is forwarded, so the "clean environment"
/// guarantee still holds for everything else.
#[cfg(windows)]
pub const POWERSHELL_ENV_KEYS: &[&str] = &[
    "SystemRoot",
    "windir",
    "ComSpec",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "COMPUTERNAME",
    "USERNAME",
    "USERDOMAIN",
    "HOMEDRIVE",
    "HOMEPATH",
    "PROGRAMDATA",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
];

/// Forward the Windows-only variables PowerShell requires into a command whose
/// environment has been cleared. No-op on unix.
#[cfg(windows)]
pub fn seed_powershell_env(cmd: &mut Command) {
    for (key, value) in powershell_env_vars() {
        cmd.env(key, value);
    }
}

/// The PowerShell boot variables present in the host environment.
///
/// Returned as (name, value) pairs so callers can seed either a
/// `std::process::Command` or a `tokio::process::Command`. Empty on unix.
#[cfg(windows)]
pub fn powershell_env_vars() -> impl Iterator<Item = (&'static str, String)> {
    POWERSHELL_ENV_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (*key, value)))
}

#[cfg(not(windows))]
pub fn powershell_env_vars() -> std::iter::Empty<(&'static str, String)> {
    std::iter::empty()
}

/// Get the platform-specific shell command.
///
/// # Returns
/// - Unix: `("sh", "-c")`
/// - Windows: `("cmd.exe", "/C")`
pub fn get_shell() -> (&'static str, &'static str) {
    #[cfg(unix)]
    {
        ("sh", "-c")
    }

    #[cfg(windows)]
    {
        ("cmd.exe", "/C")
    }
}

/// Get the platform-specific shell for scripting.
///
/// # Returns
/// - Unix: `("bash", "-c")`
/// - Windows: `("powershell.exe", "-Command")`
pub fn get_script_shell() -> (&'static str, &'static str) {
    #[cfg(unix)]
    {
        ("bash", "-c")
    }

    #[cfg(windows)]
    {
        ("powershell.exe", "-Command")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_shell() {
        let (shell, flag) = get_shell();
        assert!(!shell.is_empty());
        assert!(!flag.is_empty());
    }

    #[test]
    fn test_get_script_shell() {
        let (shell, flag) = get_script_shell();
        assert!(!shell.is_empty());
        assert!(!flag.is_empty());
    }

    #[test]
    fn test_execute_script_echo() {
        #[cfg(unix)]
        let script = "echo hello";

        #[cfg(windows)]
        let script = "Write-Output 'hello'";

        let result = execute_script(script);
        assert!(result.is_ok());
    }
}
