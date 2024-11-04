// https://github.com/moonlight-mod/moonlight/blob/main/packages/core/src/asar.ts
use binrw::prelude::*;
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
};

#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum AsarEntry {
    Directory { files: HashMap<String, AsarEntry> },

    File { offset: String, size: usize },
}

// Why is this required for binrw?
impl Default for AsarEntry {
    fn default() -> Self {
        AsarEntry::Directory {
            files: HashMap::new(),
        }
    }
}

pub type FileTree = HashMap<String, Vec<u8>>;

#[binrw::parser(reader)]
fn header_json_reader(actual_string_size: u32) -> BinResult<AsarEntry> {
    let mut buf = Vec::new();
    buf.resize(actual_string_size as usize, 0);
    reader.read_exact(&mut buf)?;
    let root_entry: AsarEntry =
        serde_json::from_slice(&buf).map_err(|e| error(reader, e.to_string()))?;
    Ok(root_entry)
}

fn walk_tree(
    entry: &AsarEntry,
    reader: &mut impl BinReaderExt,
    base: usize,
    output: &mut HashMap<String, Vec<u8>>,
    path: String,
) -> anyhow::Result<()> {
    match entry {
        AsarEntry::Directory { files } => {
            for (name, entry) in files {
                let child = if path != "" {
                    format!("{}/{}", path, name)
                } else {
                    name.clone()
                };
                walk_tree(entry, reader, base, output, child)?;
            }
        }

        AsarEntry::File { offset, size } => {
            let offset = offset
                .parse::<usize>()
                .map_err(|e| error(reader, e.to_string()))?;
            reader.seek(SeekFrom::Start(base as u64 + offset as u64))?;
            let mut data = vec![0; *size];
            reader.read_exact(&mut data)?;
            output.insert(path, data);
        }
    }

    Ok(())
}

#[binrw::parser(reader)]
fn file_tree_reader(header_string_size: u32, header_json: AsarEntry) -> BinResult<FileTree> {
    let mut output = HashMap::new();
    walk_tree(
        &header_json,
        reader,
        // In the TypeScript impl, we do `headerStringStart + headerStringSize + 4`
        // but headerStringStart will always be 8
        8 + header_string_size as usize + 4,
        &mut output,
        String::new(),
    )
    .map_err(|e| error(reader, e.to_string()))?;
    Ok(output)
}

#[derive(BinRead)]
#[allow(dead_code)]
pub struct AsarHeader {
    payload_size: u32,
    header_size: u32,

    header_string_size: u32,
    actual_string_size: u32,

    #[br(parse_with = header_json_reader, args(actual_string_size))]
    header_json: AsarEntry,

    #[br(parse_with = file_tree_reader, args(header_string_size, header_json.clone()))]
    pub file_tree: FileTree,
}

fn error(reader: &mut impl BinReaderExt, message: String) -> binrw::Error {
    if let Ok(stream_position) = reader.stream_position() {
        binrw::Error::Custom {
            pos: stream_position,
            err: Box::new(anyhow::anyhow!(message)),
        }
    } else {
        binrw::Error::Custom {
            pos: 0,
            err: Box::new(anyhow::anyhow!(message)),
        }
    }
}

pub fn parse_asar<R: Read + Seek>(reader: &mut R) -> anyhow::Result<FileTree> {
    Ok(reader.read_ne::<AsarHeader>()?.file_tree)
}
