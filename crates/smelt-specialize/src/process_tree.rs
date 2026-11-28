//! Killing a spawned process together with every process it started.
//!
//! [`std::process::Child::kill`] signals only the direct child. The programs
//! Smelt's tests and tools bound by a wall-time limit are usually launchers:
//! `cargo test`/`cargo run` start the generated binary as a grandchild, and
//! killing `cargo` alone leaves that binary running as an orphan (one spun for
//! two hours after its 60 s bound) that also keeps the pipes the caller drains
//! open, so a reader join after the kill blocks until the orphan exits.
//!
//! [`kill_tree`] kills the whole tree instead. On Linux it walks `/proc`
//! top-down, stopping each process (`SIGSTOP`) before listing its children so
//! that no process can fork a child the walk misses, then kills every stopped
//! process (`SIGKILL`) and reaps the direct child. The process group is left
//! alone on purpose: moving the child into a group of its own would hide it
//! from the terminal's Ctrl-C, orphaning the tree whenever a run is
//! interrupted. Elsewhere only the direct child is killed.

use std::{
    io,
    process::{Child, ExitStatus},
};

/// Kills `child` and, on Linux, every process descended from it, then reaps
/// `child` and returns its exit status.
///
/// A process that already exited is skipped; only failing to reap `child`
/// is an error.
///
/// # Errors
/// Returns the error of [`Child::wait`].
pub fn kill_tree(child: &mut Child) -> io::Result<ExitStatus> {
    #[cfg(target_os = "linux")]
    linux::kill_descendants_and(child);
    // Already dead after the tree kill on Linux; the only kill elsewhere.
    drop(child.kill());
    child.wait()
}

/// The `/proc` walk behind [`kill_tree`].
#[cfg(target_os = "linux")]
mod linux {
    use std::process::Child;

    use rustix::process::{Pid, Signal, kill_process};

    /// Stops `child` and its descendants top-down, then kills them all.
    pub(super) fn kill_descendants_and(child: &Child) {
        let Ok(root) = i32::try_from(child.id()) else {
            return;
        };
        let mut stopped = Vec::new();
        let mut frontier = vec![root];
        while let Some(pid) = frontier.pop() {
            if stopped.contains(&pid) {
                continue;
            }
            // Stopped first, so the children listed next are all it will have.
            signal(pid, Signal::STOP);
            stopped.push(pid);
            frontier.extend(children_of(pid));
        }
        for pid in stopped {
            signal(pid, Signal::KILL);
        }
    }

    /// Sends `sig` to `pid`, ignoring a process that is already gone.
    fn signal(pid: i32, sig: Signal) {
        if let Some(pid) = Pid::from_raw(pid) {
            // An error means the process already exited: nothing to signal.
            let _already_gone = kill_process(pid, sig);
        }
    }

    /// The live processes whose parent is `parent`, from each
    /// `/proc/<pid>/stat`.
    fn children_of(parent: i32) -> Vec<i32> {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
            .filter(|pid| {
                std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .and_then(|stat| parent_of(&stat))
                    == Some(parent)
            })
            .collect()
    }

    /// The parent pid in a `/proc/<pid>/stat` line: `pid (comm) state ppid
    /// ...`, where `comm` may itself contain spaces and parentheses, so the
    /// fields are read after its LAST `)`.
    pub(super) fn parent_of(stat: &str) -> Option<i32> {
        let (_, rest) = stat.rsplit_once(')')?;
        rest.split_whitespace().nth(1)?.parse().ok()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    use super::{kill_tree, linux::parent_of};

    /// The parent pid is read after the last `)`, past a `comm` that holds
    /// spaces and parentheses.
    #[test]
    fn parent_pid_survives_odd_command_names() {
        assert_eq!(parent_of("42 (a) b (c)) S 7 42 42 0"), Some(7));
        assert_eq!(parent_of("42 (sh) R 1 42"), Some(1));
        assert_eq!(parent_of("garbage"), None);
    }

    /// A grandchild started by the killed child dies with it: its pid is gone
    /// right after [`kill_tree`] returns, and the pipe it inherited closes, so
    /// draining the child's stdout finishes instead of waiting out the
    /// grandchild's `sleep`.
    #[test]
    fn a_grandchild_dies_with_its_parent() {
        let mut child = Command::new("sh")
            .args(["-c", "sleep 300 & echo $!; wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn sh");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut line = String::new();
        {
            use std::io::BufRead;
            std::io::BufReader::new(&mut stdout)
                .read_line(&mut line)
                .expect("read the grandchild pid");
        }
        let grandchild: i32 = line.trim().parse().expect("a pid");
        assert!(std::path::Path::new(&format!("/proc/{grandchild}")).exists());
        let started = Instant::now();
        kill_tree(&mut child).expect("reap the child");
        let mut rest = Vec::new();
        std::io::Read::read_to_end(&mut stdout, &mut rest).expect("drain stdout");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the inherited pipe stayed open: the grandchild survived"
        );
        // Reparented to init (or a subreaper) and reaped there; poll briefly.
        let gone = (0..200).any(|_| {
            let stat = std::fs::read_to_string(format!("/proc/{grandchild}/stat"));
            let dead = stat.map_or(true, |stat| {
                stat.rsplit_once(')')
                    .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
            });
            if !dead {
                std::thread::sleep(Duration::from_millis(10));
            }
            dead
        });
        assert!(gone, "the grandchild {grandchild} is still running");
    }
}
