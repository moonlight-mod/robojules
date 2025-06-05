use super::LogicResult;
use anyhow::Context;
use egui::TextBuffer;
use libdiffsitter::{
    diff::{compute_edit_script, DocumentType, Hunk},
    generate_ast_vector_data,
    input_processing::TreeSitterProcessor,
    parse::{lang_name_from_file_ext, GrammarConfig},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use two_face::re_exports::syntect::{
    highlighting::{
        FontStyle, HighlightState, Highlighter, RangedHighlightIterator, Theme, ThemeSet,
    },
    parsing::{ParseState, ScopeStack, SyntaxSet},
};

#[derive(Debug, Clone)]
pub struct ModifiedExtension {
    pub id: String,
    pub repository: String,
    pub old_commit: Option<String>,
    pub new_commit: String,
}

#[derive(Debug, Clone)]
pub struct PullRequestUpdate {
    pub extensions: Vec<ModifiedExtension>,
    pub artifact_url: String,
}

#[derive(Debug, Clone)]
pub struct DiffedExtension {
    pub id: String,
    pub source_diff: FolderDiff,
    pub asar_diff: FolderDiff,
}

#[derive(Debug, Clone)]
pub enum FileState {
    Modified,
    Added,
    Removed,
}

pub type Directory = Vec<FilesystemItem>;

#[derive(Debug, Clone)]
pub enum FilesystemItem {
    File {
        name: String,
        state: FileState,
    },
    Directory {
        name: Option<String>,
        children: Directory,
    },
}

#[derive(Debug, Clone)]
pub struct FolderDiff {
    pub old: PathBuf,
    pub new: PathBuf,
    pub dir: Directory,
}

// path/to/file -> sha256
pub async fn get_dir_tree(dir: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut tree = HashMap::new();

    let mut files = tokio::fs::read_dir(dir)
        .await
        .context("Failed to read directory")?;

    while let Some(file) = files.next_entry().await? {
        let path = file.path();
        let path_str = path.strip_prefix(dir)?.to_string_lossy().to_string();

        if path.is_dir() {
            let children = Box::pin(get_dir_tree(&path)).await?;
            for (child_path, hash) in children {
                tree.insert(format!("{}/{}", path_str, child_path), hash);
            }
        } else {
            let mut hash = Sha256::new();
            hash.update(tokio::fs::read(&path).await?);
            tree.insert(path_str, format!("{:x}", hash.finalize()));
        }
    }

    Ok(tree)
}

pub fn unflatten_tree(
    tree: &HashMap<String, FileState>,
    prefix: Option<String>,
) -> anyhow::Result<Directory> {
    let mut children: Vec<FilesystemItem> = Vec::new();

    let items = tree
        .keys()
        .filter(|path| {
            if let Some(prefix) = &prefix {
                path.starts_with(prefix)
            } else {
                true
            }
        })
        .map(|path| {
            if let Some(prefix) = &prefix {
                path.strip_prefix(format!("{}/", prefix).as_str())
                    .unwrap()
                    .to_string()
            } else {
                path.clone()
            }
        })
        .collect::<Vec<_>>();

    let files = items
        .iter()
        .filter(|path| !path.contains('/'))
        .collect::<Vec<_>>();
    let dirs = items
        .iter()
        .filter(|path| path.contains('/'))
        .map(|path| path.split('/').next().unwrap())
        .collect::<Vec<_>>();

    for dir in dirs {
        if children.iter().any(|item| match item {
            FilesystemItem::Directory { name, .. } => name.as_deref() == Some(dir),
            _ => false,
        }) {
            continue;
        }

        let path = if let Some(prefix) = &prefix {
            format!("{}/{}", prefix, dir)
        } else {
            dir.to_string()
        };

        let subtree = unflatten_tree(tree, Some(path))?;
        children.push(FilesystemItem::Directory {
            name: Some(dir.to_string()),
            children: subtree,
        });
    }

    for file in files {
        let path = if let Some(prefix) = &prefix {
            format!("{}/{}", prefix, file)
        } else {
            file.to_string()
        };
        if let Some(state) = tree.get(&path) {
            children.push(FilesystemItem::File {
                name: file.to_string(),
                state: state.clone(),
            });
        }
    }

    Ok(children)
}

pub async fn calculate_folder_diff(old_dir: &Path, new_dir: &Path) -> anyhow::Result<FolderDiff> {
    let old_tree = get_dir_tree(old_dir).await?;
    let new_tree = get_dir_tree(new_dir).await?;

    let mut tree = HashMap::new();
    for (path, old_hash) in &old_tree {
        if let Some(new_hash) = new_tree.get(&*path) {
            if *old_hash != *new_hash {
                tree.insert(path.clone(), FileState::Modified);
            }
        } else {
            tree.insert(path.clone(), FileState::Removed);
        }
    }
    for (path, _) in new_tree {
        if old_tree.get(&path).is_none() {
            tree.insert(path, FileState::Added);
        }
    }

    let file_tree = unflatten_tree(&tree, None)?;

    Ok(FolderDiff {
        old: old_dir.to_path_buf(),
        new: new_dir.to_path_buf(),
        dir: file_tree,
    })
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub old: Vec<DiffRenderFragment>,
    pub new: Vec<DiffRenderFragment>,
}

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(|| two_face::syntax::extra_newlines());
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    // https://github.com/catppuccin/bat/blob/main/themes/Catppuccin%20Mocha.tmTheme
    let bytes = include_bytes!("../../assets/catppuccin-mocha.tmTheme");
    let mut cursor = Cursor::new(bytes);
    ThemeSet::load_from_reader(&mut cursor).unwrap()
});

#[derive(Debug, Clone)]
pub enum DiffRenderCommand {
    SetHighlight(bool),

    SetBold(bool),
    SetItalic(bool),
    SetUnderline(bool),
    SetColor(egui::Color32),

    Text(String),
}

#[derive(Debug, Clone)]
pub struct DiffRenderFragment(pub usize, pub DiffRenderCommand);

fn parse_diff<'a>(hunk: &Hunk<'a>, commands: &mut Vec<DiffRenderFragment>) {
    for line in &hunk.0 {
        for entry in &line.entries {
            let bytes = entry.reference.byte_range();
            commands.push(DiffRenderFragment(
                bytes.start,
                DiffRenderCommand::SetHighlight(true),
            ));
            commands.push(DiffRenderFragment(
                bytes.end,
                DiffRenderCommand::SetHighlight(false),
            ));
        }
    }
}

async fn do_file_diffing(
    old: &Path,
    old_commands: &mut Vec<DiffRenderFragment>,
    new: &Path,
    new_commands: &mut Vec<DiffRenderFragment>,
    extension: &str,
) -> LogicResult<()> {
    let grammar = GrammarConfig::default();

    let language =
        lang_name_from_file_ext(&extension, &grammar).context("couldn't determine language")?;

    let old_data = generate_ast_vector_data(old.into(), Some(language), &grammar)?;
    let new_data = generate_ast_vector_data(new.into(), Some(language), &grammar)?;

    let processor = TreeSitterProcessor::default();
    let old_diff = processor.process(&old_data.tree, &old_data.text);
    let new_diff = processor.process(&new_data.tree, &new_data.text);

    let hunks = compute_edit_script(&old_diff, &new_diff)?;

    for wrapper in &hunks.0 {
        match wrapper {
            DocumentType::Old(hunk) => parse_diff(hunk, old_commands),
            DocumentType::New(hunk) => parse_diff(hunk, new_commands),
        }
    }

    Ok(())
}

// slightly modified LinesWithEndings to preserve index
struct LinesWithEndings<'a> {
    input: &'a str,
    consumed: usize,
}

impl<'a> LinesWithEndings<'a> {
    fn from(input: &'a str) -> Self {
        LinesWithEndings { input, consumed: 0 }
    }
}

impl<'a> Iterator for LinesWithEndings<'a> {
    type Item = (&'a str, usize);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.input.is_empty() {
            return None;
        }
        let split = self
            .input
            .find('\n')
            .map(|i| i + 1)
            .unwrap_or_else(|| self.input.len());

        let offset = self.consumed;
        let byte_index = self.input.byte_index_from_char_index(split);
        self.consumed += byte_index;

        let (line, rest) = self.input.split_at(split);
        self.input = rest;

        Some((line, offset))
    }
}

async fn do_syntax_highlighting(
    src: &str,
    extension: &str,
    commands: &mut Vec<DiffRenderFragment>,
) -> LogicResult<()> {
    let syntax = SYNTAX_SET
        .find_syntax_by_extension(&extension)
        .context("couldn't get syntax")?;

    let lines = LinesWithEndings::from(&src);

    let highlighter = Highlighter::new(&THEME);
    let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
    let mut parse_state = ParseState::new(syntax);

    let mut last_bold = false;
    let mut last_italic = false;
    let mut last_underline = false;
    let mut last_color = egui::Color32::TRANSPARENT;

    for (line, offset) in lines {
        let ops = parse_state
            .parse_line(line, &SYNTAX_SET)
            .context("failed to parse line")?;
        let iter = RangedHighlightIterator::new(&mut highlight_state, &ops[..], line, &highlighter);
        for (style, _, range) in iter {
            let start = offset + line.byte_index_from_char_index(range.start);
            //let end = offset + line.byte_index_from_char_index(range.end);

            let bold = style.font_style.contains(FontStyle::BOLD);
            if bold != last_bold {
                commands.push(DiffRenderFragment(start, DiffRenderCommand::SetBold(bold)));
                last_bold = bold;
            }

            let italic = style.font_style.contains(FontStyle::ITALIC);
            if italic != last_italic {
                commands.push(DiffRenderFragment(
                    start,
                    DiffRenderCommand::SetItalic(italic),
                ));
                last_italic = italic;
            }

            let underline = style.font_style.contains(FontStyle::UNDERLINE);
            if underline != last_underline {
                commands.push(DiffRenderFragment(
                    start,
                    DiffRenderCommand::SetUnderline(underline),
                ));
                last_underline = underline;
            }

            let color = egui::Color32::from_rgba_premultiplied(
                style.foreground.r,
                style.foreground.g,
                style.foreground.b,
                style.foreground.a,
            );
            if color != last_color {
                commands.push(DiffRenderFragment(
                    start,
                    DiffRenderCommand::SetColor(color),
                ));
                last_color = color;
            }
        }
    }

    Ok(())
}

fn cleanup_commands(
    src: &str,
    raw_commands: &mut Vec<DiffRenderFragment>,
) -> Vec<DiffRenderFragment> {
    let mut new_commands = Vec::new();
    let mut pos = 0;
    let size = src.len();

    raw_commands.sort_by(|a, b| a.0.cmp(&b.0));
    let mut iter = raw_commands.iter();
    while let Some(fragment) = iter.next() {
        // add the line of text up to now
        if fragment.0 > pos {
            new_commands.push(DiffRenderFragment(
                pos,
                DiffRenderCommand::Text(src.char_range(pos..fragment.0).to_string()),
            ));
            pos = fragment.0;
        }

        // weave command in between the text
        new_commands.push(fragment.clone());
    }

    // add remaining text in the file
    if pos < size {
        new_commands.push(DiffRenderFragment(
            pos,
            DiffRenderCommand::Text(src.char_range(pos..size).to_string()),
        ));
    }

    new_commands
}

pub async fn calculate_file_diff_inner(
    old: &Path,
    old_str: &str,
    new: &Path,
    new_str: &str,
    extension: &str,
) -> LogicResult<FileDiff> {
    let mut old_commands = Vec::new();
    let mut new_commands = Vec::new();

    if let Err(e) = do_syntax_highlighting(&old_str, &extension, &mut old_commands).await {
        log::error!("Error highlighting old file: {}", e);
    }
    if let Err(e) = do_syntax_highlighting(&new_str, &extension, &mut new_commands).await {
        log::error!("Error highlighting new file: {}", e);
    }
    if let Err(e) =
        do_file_diffing(old, &mut old_commands, new, &mut new_commands, &extension).await
    {
        log::error!("Error diffing files: {}", e);
    }

    Ok(FileDiff {
        old: cleanup_commands(&old_str, &mut old_commands),
        new: cleanup_commands(&new_str, &mut new_commands),
    })
}

pub async fn highlight_single_file(
    file: &Path,
    extension: &str,
) -> LogicResult<Vec<DiffRenderFragment>> {
    let src = tokio::fs::read_to_string(file)
        .await
        .context("couldn't read file")?;

    let mut commands = Vec::new();

    if let Err(e) = do_syntax_highlighting(&src, &extension, &mut commands).await {
        log::error!("Error highlighting file, falling back to raw text: {}", e);
        return Ok(vec![DiffRenderFragment(0, DiffRenderCommand::Text(src))]);
    }

    Ok(cleanup_commands(&src, &mut commands))
}

pub async fn calculate_file_diff(old: &Path, new: &Path) -> LogicResult<FileDiff> {
    let extension = new.extension().context("no file extension")?;
    let extension = extension.to_string_lossy();

    if !old.exists() {
        return Ok(FileDiff {
            old: Vec::new(),
            new: highlight_single_file(new, &extension).await?,
        });
    }

    if !new.exists() {
        return Ok(FileDiff {
            old: highlight_single_file(old, &extension).await?,
            new: Vec::new(),
        });
    }

    let old_str = tokio::fs::read_to_string(old)
        .await
        .context("couldn't read old file")?;
    let new_str = tokio::fs::read_to_string(new)
        .await
        .context("couldn't read new file")?;

    let result = calculate_file_diff_inner(old, &old_str, new, &new_str, &extension).await;
    if let Ok(diff) = result {
        return Ok(diff);
    } else {
        log::error!("Failed to diff, falling back: {}", result.unwrap_err());
        return Ok(FileDiff {
            old: vec![DiffRenderFragment(0, DiffRenderCommand::Text(old_str))],
            new: vec![DiffRenderFragment(0, DiffRenderCommand::Text(new_str))],
        });
    }
}
