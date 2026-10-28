use super::Task;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct State {
    file: Option<File>,
    end: u64,
    closed: bool,
}

/// An unlinked, owner-only append log with on-disk links for session FIFOs.
/// Only head and tail offsets for currently active sessions are kept in memory.
pub(super) struct Spool {
    state: Mutex<State>,
    pub(super) changed: Notify,
}

pub(super) struct Writer(pub(super) Arc<Spool>);

pub(super) struct Record {
    pub(super) next: u64,
    pub(super) link: Option<u64>,
    pub(super) task: (usize, Task),
}

impl Spool {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                file: None,
                end: 0,
                closed: false,
            }),
            changed: Notify::new(),
        })
    }

    fn create_file() -> Result<File, String> {
        let dir = std::env::temp_dir();
        for _ in 0..128 {
            let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(".xstack-subtasks-{}-{id}", std::process::id()));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(file) => {
                    if let Err(error) = std::fs::remove_file(&path) {
                        drop(file);
                        return Err(format!("cannot unlink private task spool: {error}"));
                    }
                    return Ok(file);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("cannot create private task spool: {error}")),
            }
        }
        Err("cannot create private task spool: too many name collisions".into())
    }

    fn append(&self, index: usize, task: &Task) -> Result<(), String> {
        let data = serde_json::to_vec(&(index, task))
            .map_err(|error| format!("cannot encode task spool record: {error}"))?;
        let mut state = self.state.lock().expect("task spool");
        if state.file.is_none() {
            state.file = Some(Self::create_file()?);
        }
        let end = state.end;
        let file = state.file.as_mut().expect("task spool file");
        let result = (|| -> std::io::Result<()> {
            file.seek(SeekFrom::Start(end))?;
            file.write_all(&(data.len() as u64).to_le_bytes())?;
            file.write_all(&u64::MAX.to_le_bytes())?;
            file.write_all(&data)?;
            Ok(())
        })();
        result.map_err(|error| format!("cannot write private task spool: {error}"))?;
        state.end = end
            .checked_add(16)
            .and_then(|n| n.checked_add(data.len() as u64))
            .ok_or_else(|| "private task spool is too large".to_string())?;
        drop(state);
        self.changed.notify_one();
        Ok(())
    }

    pub(super) fn read(&self, offset: u64) -> Result<(Option<Record>, bool), String> {
        let mut state = self.state.lock().expect("task spool");
        if offset == state.end {
            return Ok((None, state.closed));
        }
        let end = state.end;
        let file = state.file.as_mut().expect("task spool file");
        let result = (|| -> std::io::Result<Record> {
            file.seek(SeekFrom::Start(offset))?;
            let mut header = [0; 16];
            file.read_exact(&mut header)?;
            let len = u64::from_le_bytes(header[..8].try_into().expect("record length"));
            let link = u64::from_le_bytes(header[8..].try_into().expect("record link"));
            let next = offset
                .checked_add(16)
                .and_then(|n| n.checked_add(len))
                .filter(|next| *next <= end)
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid record length")
                })?;
            let len = usize::try_from(len).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "record too large")
            })?;
            let mut data = vec![0; len];
            file.read_exact(&mut data)?;
            let task = serde_json::from_slice(&data).map_err(std::io::Error::other)?;
            Ok(Record {
                next,
                link: (link != u64::MAX).then_some(link),
                task,
            })
        })();
        result
            .map(|record| (Some(record), state.closed))
            .map_err(|error| format!("cannot read private task spool: {error}"))
    }

    pub(super) fn link(&self, previous: u64, next: u64) -> Result<(), String> {
        let mut state = self.state.lock().expect("task spool");
        let file = state.file.as_mut().expect("task spool file");
        file.seek(SeekFrom::Start(previous + 8))
            .and_then(|_| file.write_all(&next.to_le_bytes()))
            .map_err(|error| format!("cannot update private task spool: {error}"))
    }

    pub(super) fn should_reclaim(&self, cursor: u64, live: u64) -> Result<bool, String> {
        let discarded = cursor
            .checked_sub(live)
            .ok_or_else(|| "invalid private task spool accounting".to_string())?;
        if discarded < 4 * 1024 * 1024 {
            return Ok(false);
        }
        let state = self.state.lock().expect("task spool");
        Ok(live.saturating_add(state.end - cursor) <= discarded)
    }

    pub(super) fn reclaim_with_sessions(
        &self,
        cursor: u64,
        live: u64,
        sessions: &mut std::collections::HashMap<String, super::SessionQueue>,
        cancel: &tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<u64>, String> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        let discarded = cursor
            .checked_sub(live)
            .ok_or_else(|| "invalid private task spool accounting".to_string())?;
        let mut state = self.state.lock().expect("task spool");
        let remaining = state.end - cursor;
        if discarded < 4 * 1024 * 1024 || live.saturating_add(remaining) > discarded {
            return Ok(None);
        }
        let file = state.file.as_mut().expect("task spool file");
        let mut offsets: Vec<_> = sessions
            .iter()
            .map(|(name, queue)| (name.clone(), queue.head, None::<u64>, None::<u64>))
            .collect();
        let mut pending = BinaryHeap::new();
        for (index, (_, head, _, _)) in offsets.iter().enumerate() {
            if let Some(head) = head {
                pending.push(Reverse((*head, index)));
            }
        }
        let mut end = 0u64;
        let copy = (|| -> std::io::Result<()> {
            let mut buffer = [0u8; 64 * 1024];
            while let Some(Reverse((position, index))) = pending.pop() {
                if *cancel.borrow() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "task spool compaction cancelled",
                    ));
                }
                file.seek(SeekFrom::Start(position))?;
                let mut header = [0u8; 16];
                file.read_exact(&mut header)?;
                let length = u64::from_le_bytes(header[..8].try_into().unwrap());
                let next = u64::from_le_bytes(header[8..].try_into().unwrap());
                let record_end = position
                    .checked_add(16)
                    .and_then(|n| n.checked_add(length))
                    .filter(|&n| n <= cursor)
                    .ok_or_else(|| std::io::Error::other("invalid deferred task record"))?;
                if end > position {
                    return Err(std::io::Error::other("invalid deferred task ordering"));
                }
                if let Some(previous) = offsets[index].3 {
                    file.seek(SeekFrom::Start(previous + 8))?;
                    file.write_all(&end.to_le_bytes())?;
                } else {
                    offsets[index].2 = Some(end);
                }
                file.seek(SeekFrom::Start(end))?;
                file.write_all(&length.to_le_bytes())?;
                file.write_all(&u64::MAX.to_le_bytes())?;
                let mut copied = 0u64;
                while copied < length {
                    if *cancel.borrow() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "task spool compaction cancelled",
                        ));
                    }
                    let count = (length - copied).min(buffer.len() as u64) as usize;
                    file.seek(SeekFrom::Start(position + 16 + copied))?;
                    file.read_exact(&mut buffer[..count])?;
                    file.seek(SeekFrom::Start(end + 16 + copied))?;
                    file.write_all(&buffer[..count])?;
                    copied += count as u64;
                }
                offsets[index].3 = Some(end);
                end += record_end - position;
                if next != u64::MAX {
                    if next < record_end || next >= cursor {
                        return Err(std::io::Error::other("invalid deferred task link"));
                    }
                    pending.push(Reverse((next, index)));
                }
            }
            let mut copied = 0u64;
            while copied < remaining {
                if *cancel.borrow() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "task spool compaction cancelled",
                    ));
                }
                let count = (remaining - copied).min(buffer.len() as u64) as usize;
                file.seek(SeekFrom::Start(cursor + copied))?;
                file.read_exact(&mut buffer[..count])?;
                file.seek(SeekFrom::Start(end + copied))?;
                file.write_all(&buffer[..count])?;
                copied += count as u64;
            }
            file.set_len(end + remaining)?;
            Ok(())
        })();
        match copy {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted && *cancel.borrow() => {
                return Ok(None)
            }
            Err(error) => return Err(format!("cannot reclaim private task spool: {error}")),
            Ok(()) => {}
        }
        state.end = end + remaining;
        for (name, _, head, tail) in offsets {
            let queue = sessions.get_mut(&name).expect("active browser session");
            queue.head = head;
            queue.tail = tail;
        }
        Ok(Some(end))
    }
}

impl Writer {
    pub(super) fn append(&self, index: usize, task: &Task) -> Result<(), String> {
        self.0.append(index, task)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.0.state.lock().expect("task spool").closed = true;
        self.0.changed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Browser, SessionQueue};
    use std::collections::HashMap;

    #[test]
    fn reclaim_preserves_deferred_and_unread_tasks() {
        let spool = Spool::new();
        let task = |session: &str, payload: String| {
            Task::Browser(Browser {
                session: session.into(),
                commands: vec![vec!["open".into(), payload]],
            })
        };
        spool.append(0, &task("waiting", "first".into())).unwrap();
        let first = spool.read(0).unwrap().0.unwrap();
        spool
            .append(1, &task("finished", "x".repeat(4 * 1024 * 1024)))
            .unwrap();
        let finished = spool.read(first.next).unwrap().0.unwrap();
        spool.append(2, &task("waiting", "second".into())).unwrap();
        let second_offset = finished.next;
        let second = spool.read(second_offset).unwrap().0.unwrap();
        spool.link(0, second_offset).unwrap();
        let cursor = second.next;
        spool.append(3, &task("unread", "last".into())).unwrap();
        let mut sessions = HashMap::from([(
            "waiting".to_string(),
            SessionQueue {
                head: Some(0),
                tail: Some(second_offset),
            },
        )]);
        let live = first.next + second.next - second_offset;
        let (cancel_tx, cancel) = tokio::sync::watch::channel(false);
        cancel_tx.send(true).unwrap();
        assert!(spool
            .reclaim_with_sessions(cursor, live, &mut sessions, &cancel)
            .unwrap()
            .is_none());
        cancel_tx.send(false).unwrap();
        let new_cursor = spool
            .reclaim_with_sessions(cursor, live, &mut sessions, &cancel)
            .unwrap()
            .unwrap();
        let head = sessions["waiting"].head.unwrap();
        let first = spool.read(head).unwrap().0.unwrap();
        let second = spool.read(first.link.unwrap()).unwrap().0.unwrap();
        let unread = spool.read(new_cursor).unwrap().0.unwrap();
        assert_eq!((first.task.0, second.task.0, unread.task.0), (0, 2, 3));
        assert_eq!(sessions["waiting"].tail, first.link);
        assert!(spool.state.lock().unwrap().end < 1024);
    }
}
