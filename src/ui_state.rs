//! Persistent presentation settings and pure navigation helpers.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Columns {
    pub kind: bool,
    pub size: bool,
    pub modified: bool,
    pub metadata_in_dual: bool,
    pub kind_width: f32,
    pub size_width: f32,
    pub modified_width: f32,
}
impl Default for Columns {
    fn default() -> Self {
        Self {
            kind: true,
            size: true,
            modified: true,
            metadata_in_dual: true,
            kind_width: 130.0,
            size_width: 100.0,
            modified_width: 145.0,
        }
    }
}
impl Columns {
    pub fn widths(&self, width: f32, compact: bool) -> [f32; 4] {
        let usable = (width - 28.0).max(80.0);
        let mut cols = [0.0; 4];
        cols[1] = if self.kind && !compact {
            self.kind_width.clamp(65.0, 220.0)
        } else {
            0.0
        };
        cols[2] = if self.size {
            self.size_width.clamp(65.0, 180.0)
        } else {
            0.0
        };
        cols[3] = if self.modified && !compact {
            self.modified_width.clamp(120.0, 220.0)
        } else {
            0.0
        };
        let budget = (usable - 100.0).max(0.0);
        let sum = cols[1] + cols[2] + cols[3];
        if sum > budget && sum > 0.0 {
            for c in &mut cols[1..] {
                *c *= budget / sum;
            }
        }
        cols[0] = usable - cols[1] - cols[2] - cols[3];
        cols
    }
    pub fn resize(&mut self, index: usize, delta: f32) {
        match index {
            1 => self.kind_width = (self.kind_width + delta).clamp(65.0, 220.0),
            2 => self.size_width = (self.size_width + delta).clamp(65.0, 180.0),
            3 => self.modified_width = (self.modified_width + delta).clamp(120.0, 220.0),
            _ => {}
        }
    }
}
pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_owned()
    }
}
pub fn step_index(current: Option<usize>, delta: isize, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match current {
        None => {
            if delta < 0 {
                count - 1
            } else {
                0
            }
        }
        Some(i) => i.saturating_add_signed(delta).min(count - 1),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resizing_and_visibility_stay_inside_viewport() {
        let mut c = Columns::default();
        c.resize(1, 500.0);
        for w in [220.0, 450.0, 900.0, 1900.0] {
            let a = c.widths(w, w < 650.0);
            assert!(a.iter().all(|v| v.is_finite() && *v >= 0.0));
            assert!(a.iter().sum::<f32>() <= w);
        }
        c.kind = false;
        assert_eq!(c.widths(900.0, false)[1], 0.0);
    }
    #[test]
    fn keyboard_edges() {
        assert_eq!(step_index(None, 1, 3), Some(0));
        assert_eq!(step_index(Some(0), -1, 3), Some(0));
        assert_eq!(step_index(None, 1, 0), None);
    }
    #[test]
    fn technical_windows_prefix_is_only_removed_for_display() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Users\Test")),
            r"C:\Users\Test"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share")),
            r"\\server\share"
        );
    }
}
