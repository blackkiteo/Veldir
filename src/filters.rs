//! Fast, deterministic, in-memory filters for a single directory.
//! Folders remain visible (subject to the name filter) so navigation remains usable.
use crate::fs_ops::Entry;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileKind {
    #[default]
    All, Documents, Images, Archives, Video, Audio, Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeRange {
    #[default]
    All, Small, Medium, Large,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModifiedRange {
    #[default]
    All, Day, Week, Month,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Criteria {
    pub kind: FileKind,
    pub size: SizeRange,
    pub modified: ModifiedRange,
}

impl Criteria {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn category_matches(ext: &str, kind: FileKind) -> bool {
    let is_document = matches!(ext,
        "pdf"|"doc"|"docx"|"xls"|"xlsx"|"ppt"|"pptx"|
        "txt"|"md"|"rtf"|"odt"|"ods"|"csv"|"json"|"xml");
    let is_image = matches!(ext,"jpg"|"jpeg"|"png"|"bmp"|"webp"|"gif"|"tiff"|"svg"|"heic"|"raw");
    let is_archive = matches!(ext,"zip"|"rar"|"7z"|"tar"|"gz"|"bz2"|"xz");
    let is_video = matches!(ext,"mp4"|"mkv"|"mov"|"avi"|"webm"|"wmv"|"m4v");
    let is_audio = matches!(ext,"mp3"|"wav"|"flac"|"aac"|"ogg"|"m4a"|"wma");
    match kind {
        FileKind::All => true,
        FileKind::Documents => is_document,
        FileKind::Images => is_image,
        FileKind::Archives => is_archive,
        FileKind::Video => is_video,
        FileKind::Audio => is_audio,
        FileKind::Other => !(is_document||is_image||is_archive||is_video||is_audio),
    }
}

/// Sizes use binary units. Thresholds: <1 MiB, 1–100 MiB, >=100 MiB.
pub fn matches(entry: &Entry, name_filter: &str, criteria: Criteria, now: SystemTime) -> bool {
    if !name_filter.is_empty() && !entry.name.to_lowercase().contains(name_filter) {
        return false;
    }
    if entry.is_dir { return true; }
    let ext=entry.path.extension().and_then(|x|x.to_str())
        .unwrap_or("").to_ascii_lowercase();
    if !category_matches(&ext,criteria.kind) { return false; }
    const MIB:u64=1024*1024;
    match criteria.size {
        SizeRange::All=>{},
        SizeRange::Small if entry.size>=MIB=>return false,
        SizeRange::Medium if entry.size<MIB||entry.size>=100*MIB=>return false,
        SizeRange::Large if entry.size<100*MIB=>return false,
        _=>{}
    }
    let max_age=match criteria.modified {
        ModifiedRange::All=>None,
        ModifiedRange::Day=>Some(Duration::from_secs(86_400)),
        ModifiedRange::Week=>Some(Duration::from_secs(7*86_400)),
        ModifiedRange::Month=>Some(Duration::from_secs(30*86_400)),
    };
    if let Some(limit)=max_age {
        let Some(modified)=entry.modified else {return false};
        if now.duration_since(modified).unwrap_or_default()>limit {return false;}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn file(name:&str,size:u64,age_days:u64)->Entry {
        Entry {
            name:name.into(),path:PathBuf::from(name),is_dir:false,size,
            modified:Some(SystemTime::now()-Duration::from_secs(age_days*86_400)),
        }
    }
    #[test]
    fn type_size_and_date_are_combined() {
        let now=SystemTime::now();
        let pdf=file("ANNUALE.PDF",20*1024*1024,3);
        let q=Criteria {kind:FileKind::Documents,size:SizeRange::Medium,modified:ModifiedRange::Week};
        assert!(matches(&pdf,"ann",q,now));
        assert!(!matches(&pdf,"invalid",q,now));
        assert!(!matches(&pdf,"ann",Criteria{modified:ModifiedRange::Day,..q},now));
        assert!(!matches(&pdf,"ann",Criteria{size:SizeRange::Small,..q},now));
        assert!(!matches(&file("photo.jpg",20*1024*1024,3),"",q,now));
    }
    #[test]
    fn folders_stay_navigable_and_missing_modification_is_safe() {
        let mut folder=file("Photos",0,99);folder.is_dir=true;folder.modified=None;
        assert!(matches(&folder,"phot",Criteria{kind:FileKind::Documents,
            size:SizeRange::Large,modified:ModifiedRange::Day},SystemTime::now()));
        let unknown=file("missing.txt",1,2);
        assert!(!matches(&Entry{modified:None,..unknown},"",Criteria{
            modified:ModifiedRange::Week,..Criteria::default()
        },SystemTime::now()));
    }
    #[test]
    fn edge_thresholds_follow_binary_mib() {
        let now=SystemTime::now();
        let one=file("a.pdf",1024*1024,0);
        assert!(!matches(&one,"",Criteria{size:SizeRange::Small,..Criteria::default()},now));
        assert!(matches(&one,"",Criteria{size:SizeRange::Medium,..Criteria::default()},now));
    }
}
