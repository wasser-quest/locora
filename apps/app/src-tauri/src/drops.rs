//! Short-lived, single-use authority for files delivered by the native WebView.
//! The frontend receives a ticket, never a filesystem path.
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Inbox {
    sequence: u64,
    pending: Option<(String, PathBuf, Instant)>,
}

impl Inbox {
    pub fn offer(&mut self, paths: &[PathBuf]) -> Result<String, &'static str> {
        self.pending = None;
        if paths.len() != 1 {
            return Err("Bitte genau eine Bild- oder Videodatei hineinziehen");
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Bitte die App neu starten")?;
        let ticket = self.sequence.to_string();
        self.pending = Some((ticket.clone(), paths[0].clone(), Instant::now()));
        Ok(ticket)
    }

    pub fn take(&mut self, ticket: &str) -> Result<PathBuf, &'static str> {
        let Some((expected, _, created)) = self.pending.as_ref() else {
            return Err("Keine hineingezogene Datei verfügbar");
        };
        if created.elapsed() > Duration::from_secs(30) {
            self.pending = None;
            return Err("Datei bitte erneut hineinziehen");
        }
        if expected != ticket {
            return Err("Ungültige Dateifreigabe");
        }
        Ok(self.pending.take().unwrap().1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tickets_are_single_use_and_cannot_supply_paths() {
        let mut inbox = Inbox::default();
        let path = PathBuf::from("/tmp/chosen.png");
        let ticket = inbox.offer(std::slice::from_ref(&path)).unwrap();
        assert!(inbox.take("/etc/passwd").is_err());
        assert_eq!(inbox.take(&ticket).unwrap(), path);
        assert!(inbox.take(&ticket).is_err());
    }
    #[test]
    fn new_drop_revokes_old_authority_and_invalid_batch_revokes_all() {
        let mut inbox = Inbox::default();
        let paths = [PathBuf::from("a.png")];
        let old = inbox.offer(&paths).unwrap();
        let new = inbox.offer(&paths).unwrap();
        assert!(inbox.take(&old).is_err());
        assert!(inbox.offer(&[]).is_err());
        assert!(inbox.take(&new).is_err());
    }
    #[test]
    fn expired_ticket_cannot_read_file() {
        let mut inbox = Inbox::default();
        let ticket = inbox.offer(&[PathBuf::from("a.png")]).unwrap();
        inbox.pending.as_mut().unwrap().2 = Instant::now() - Duration::from_secs(31);
        assert!(inbox.take(&ticket).is_err());
        assert!(inbox.pending.is_none());
    }
}
