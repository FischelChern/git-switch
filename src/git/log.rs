//! 提交历史与引用，输出可直接喂给 [`crate::model::graph::layout`]。

use std::collections::HashMap;
use std::path::Path;

use thiserror::Error;

use crate::model::graph::{CommitNode, GraphLayout, layout};

use super::exec::{GitError, GitExecutor};

/// 记录分隔符（RS）。
const RS: char = '\u{1e}';
/// 字段分隔符（US）。
const US: char = '\u{1f}';

/// 一条提交。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub summary: String,
    /// 指向该提交的引用（分支、标签）。
    pub refs: Vec<String>,
}

impl Commit {
    /// 短 id，展示用。
    pub fn short_id(&self) -> &str {
        let n = self.id.len().min(7);
        &self.id[..n]
    }

    /// 是否合并提交。
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

/// 提交图：提交列表 + 泳道布局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphData {
    pub commits: Vec<Commit>,
    pub layout: GraphLayout,
}

/// 读取错误。
#[derive(Debug, Error)]
pub enum LogError {
    #[error(transparent)]
    Git(#[from] GitError),
}

/// 解析 `--pretty=format:%H<US>%P<US>%an<US>%ad<US>%s<RS>` 的输出。
pub fn parse_log(input: &str) -> Vec<Commit> {
    input
        .split(RS)
        .map(str::trim_start)
        .filter(|s| !s.is_empty())
        .filter_map(|record| {
            let mut fields = record.split(US);
            let id = fields.next()?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            let parents = fields
                .next()
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let author = fields.next().unwrap_or("").to_string();
            let date = fields.next().unwrap_or("").to_string();
            let summary = fields.next().unwrap_or("").to_string();
            Some(Commit {
                id,
                parents,
                author,
                date,
                summary,
                refs: Vec::new(),
            })
        })
        .collect()
}

/// 解析 `git for-each-ref --format=%(objectname)<US>%(refname:short)` 的输出。
pub fn parse_refs(input: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split(US);
        let Some(oid) = it.next() else { continue };
        let Some(name) = it.next() else { continue };
        if oid.is_empty() || name.is_empty() {
            continue;
        }
        map.entry(oid.to_string())
            .or_default()
            .push(name.to_string());
    }
    map
}

/// 读取提交历史（含全部引用），并算好泳道布局。
pub fn read_graph(repo: &Path, limit: usize) -> Result<GraphData, LogError> {
    let ex = GitExecutor::new(repo);
    let limit_arg = format!("-n{limit}");
    let format_arg = format!("--pretty=format:%H{US}%P{US}%an{US}%ad{US}%s{RS}");

    let out = ex.run(&[
        "log",
        "--all",
        "--date-order",
        "--date=short",
        &limit_arg,
        &format_arg,
    ])?;
    if !out.success {
        return Err(LogError::Git(GitError::CommandFailed {
            command: out.command,
            stderr: out.stderr,
        }));
    }

    let mut commits = parse_log(&out.stdout);

    // 引用（分支 / 标签）
    if let Ok(refs_out) = ex.run(&[
        "for-each-ref",
        &format!("--format=%(objectname){US}%(refname:short)"),
    ]) {
        if refs_out.success {
            let refs = parse_refs(&refs_out.stdout);
            for c in &mut commits {
                if let Some(names) = refs.get(&c.id) {
                    c.refs = names.clone();
                }
            }
        }
    }

    let nodes: Vec<CommitNode> = commits
        .iter()
        .map(|c| CommitNode::new(c.id.clone(), c.parents.clone()))
        .collect();
    let layout = layout(&nodes);

    Ok(GraphData { commits, layout })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, parents: &str, author: &str, date: &str, summary: &str) -> String {
        format!("{id}{US}{parents}{US}{author}{US}{date}{US}{summary}{RS}")
    }

    #[test]
    fn parses_single_commit() {
        let input = record("abc123", "", "张鑫宇", "2026-11-26", "初始化");
        let commits = parse_log(&input);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].id, "abc123");
        assert!(commits[0].parents.is_empty());
        assert_eq!(commits[0].author, "张鑫宇");
        assert_eq!(commits[0].summary, "初始化");
        assert!(!commits[0].is_merge());
    }

    #[test]
    fn parses_merge_with_two_parents() {
        let input = record("m1", "p1 p2", "王工", "2026-11-25", "Merge branch 'x'");
        let commits = parse_log(&input);
        assert_eq!(commits[0].parents, vec!["p1", "p2"]);
        assert!(commits[0].is_merge());
    }

    #[test]
    fn parses_multiple_records_in_order() {
        let mut input = record("c2", "c1", "a", "2026-11-26", "second");
        input.push_str(&record("c1", "", "a", "2026-11-25", "first"));
        let commits = parse_log(&input);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "c2");
        assert_eq!(commits[1].id, "c1");
    }

    #[test]
    fn summary_with_separators_like_text_is_kept() {
        let input = record("c1", "", "a", "2026-11-26", "fix: a:b (c)");
        let commits = parse_log(&input);
        assert_eq!(commits[0].summary, "fix: a:b (c)");
    }

    #[test]
    fn empty_and_garbage_input_are_ignored() {
        assert!(parse_log("").is_empty());
        assert!(parse_log(&RS.to_string()).is_empty());
        assert!(parse_log("no-separators-here").len() == 1);
    }

    #[test]
    fn short_id_is_seven_chars() {
        let input = record("0123456789abcdef", "", "a", "d", "s");
        let commits = parse_log(&input);
        assert_eq!(commits[0].short_id(), "0123456");
    }

    #[test]
    fn parses_refs_into_map() {
        let input = format!("aaa{US}main\naaa{US}v1.0\nbbb{US}origin/dev\n");
        let map = parse_refs(&input);
        assert_eq!(map.get("aaa").map(|v| v.len()), Some(2));
        assert_eq!(map.get("bbb"), Some(&vec!["origin/dev".to_string()]));
    }

    #[test]
    fn empty_refs_yield_empty_map() {
        assert!(parse_refs("").is_empty());
        assert!(parse_refs("\n\n").is_empty());
    }
}
