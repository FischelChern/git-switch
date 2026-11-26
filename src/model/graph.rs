//! 提交图的泳道布局。**纯计算，无 IO**，可完全单测。
//!
//! 输入是按显示顺序排好的提交（新 → 旧）及其父提交 id，
//! 输出每一行所在的泳道与经过该行的区间，供 UI 画线。

/// 一个提交节点的最小信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitNode {
    /// 提交 id。
    pub id: String,
    /// 父提交 id，顺序即 first-parent 优先。
    pub parents: Vec<String>,
}

impl CommitNode {
    /// 新建节点。
    pub fn new(id: impl Into<String>, parents: Vec<String>) -> Self {
        Self {
            id: id.into(),
            parents,
        }
    }
}

/// 一行的布局结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// 该提交占用的泳道。
    pub lane: usize,
    /// 本行之前仍活跃的泳道数。
    pub lanes_before: usize,
    /// 本行之后仍活跃的泳道数。
    pub lanes_after: usize,
    /// 父提交所在的泳道（第 0 个对应同泳道直连）。
    pub parent_lanes: Vec<usize>,
    /// 本行是否有侧分支汇入（用于画曲线）。
    pub merges_in: Vec<usize>,
}

/// 布局结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLayout {
    /// 逐行结果，与输入等长。
    pub rows: Vec<GraphRow>,
    /// 需要绘制的泳道总数。
    pub lane_count: usize,
}

/// 计算泳道布局。
///
/// 规则：
/// - 每个提交优先复用「期待它出现」的泳道；
/// - 否则占用第一个空泳道；
/// - 第一个父提交沿用本行泳道，其余父提交分配新泳道；
/// - 其他仍在等待本提交的泳道在本行收束。
pub fn layout(commits: &[CommitNode]) -> GraphLayout {
    // lanes[i] = 该泳道正在等待的提交 id
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let lanes_before = lanes.iter().filter(|l| l.is_some()).count();

        // 找到等待本提交的泳道
        let mut lane = lanes
            .iter()
            .position(|l| l.as_deref() == Some(commit.id.as_str()));

        if lane.is_none() {
            // 分配空泳道，没有就新增
            lane = lanes.iter().position(Option::is_none);
            if lane.is_none() {
                lanes.push(None);
                lane = Some(lanes.len() - 1);
            }
        }
        let lane = lane.unwrap_or(0);

        // 其他等待同一提交的泳道在此收束
        let mut merges_in = Vec::new();
        for (i, slot) in lanes.iter_mut().enumerate() {
            if i != lane && slot.as_deref() == Some(commit.id.as_str()) {
                *slot = None;
                merges_in.push(i);
            }
        }

        // 分配父提交
        let mut parent_lanes = Vec::with_capacity(commit.parents.len());
        if let Some(first) = commit.parents.first() {
            lanes[lane] = Some(first.clone());
            parent_lanes.push(lane);
        } else {
            lanes[lane] = None;
        }

        for parent in commit.parents.iter().skip(1) {
            // 该父提交已在别的泳道等待则复用
            let existing = lanes
                .iter()
                .position(|l| l.as_deref() == Some(parent.as_str()));
            let pl = match existing {
                Some(p) => p,
                None => {
                    let free = lanes.iter().position(Option::is_none);
                    let p = match free {
                        Some(p) => p,
                        None => {
                            lanes.push(None);
                            lanes.len() - 1
                        }
                    };
                    lanes[p] = Some(parent.clone());
                    p
                }
            };
            if !parent_lanes.contains(&pl) {
                parent_lanes.push(pl);
            }
        }

        // 收掉尾部空泳道，保持图上不出现空列
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }

        let lanes_after = lanes.iter().filter(|l| l.is_some()).count();
        rows.push(GraphRow {
            lane,
            lanes_before,
            lanes_after,
            parent_lanes,
            merges_in,
        });
    }

    let lane_count = rows
        .iter()
        .map(|r| r.lane + 1)
        .max()
        .unwrap_or(1)
        .max(
            rows.iter()
                .map(|r| {
                    r.parent_lanes
                        .iter()
                        .copied()
                        .max()
                        .map(|m| m + 1)
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0),
        )
        .max(1);

    GraphLayout { rows, lane_count }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str, parents: &[&str]) -> CommitNode {
        CommitNode::new(id, parents.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn linear_history_stays_in_one_lane() {
        let commits = vec![n("c", &["b"]), n("b", &["a"]), n("a", &[])];
        let g = layout(&commits);
        assert_eq!(g.lane_count, 1);
        assert!(g.rows.iter().all(|r| r.lane == 0));
        assert!(g.rows.iter().all(|r| r.merges_in.is_empty()));
    }

    #[test]
    fn root_commit_releases_the_lane() {
        let commits = vec![n("b", &["a"]), n("a", &[])];
        let g = layout(&commits);
        assert_eq!(g.rows[1].lanes_after, 0);
    }

    #[test]
    fn side_branch_gets_its_own_lane_and_merges_back() {
        // 显示顺序：m 合并了 A、B；A、B 都基于 base
        let commits = vec![
            n("m", &["A", "B"]),
            n("A", &["base"]),
            n("B", &["base"]),
            n("base", &[]),
        ];
        let g = layout(&commits);
        assert_eq!(g.rows[0].lane, 0, "合并提交在主泳道");
        assert_eq!(g.rows[0].parent_lanes, vec![0, 1], "第二个父提交占新泳道");
        assert_eq!(g.rows[1].lane, 0);
        assert_eq!(g.rows[2].lane, 1);
        // base 收束：应当出现 merges_in
        assert_eq!(g.rows[3].lane, 0);
        assert!(
            !g.rows[3].merges_in.is_empty(),
            "base 应从侧分支泳道收束：{:?}",
            g.rows[3]
        );
        assert_eq!(g.lane_count, 2);
    }

    #[test]
    fn octopus_merge_allocates_extra_lanes() {
        let commits = vec![
            n("m", &["a", "b", "c"]),
            n("a", &["base"]),
            n("b", &["base"]),
            n("c", &["base"]),
            n("base", &[]),
        ];
        let g = layout(&commits);
        assert_eq!(g.rows[0].parent_lanes.len(), 3);
        assert_eq!(g.lane_count, 3);
    }

    #[test]
    fn lane_is_reused_when_parent_already_pending() {
        // B 已经在泳道 1 等待 base，A 的父也是 base，应复用而不是再开一条
        let commits = vec![
            n("m", &["A", "B"]),
            n("B", &["base"]),
            n("A", &["base"]),
            n("base", &[]),
        ];
        let g = layout(&commits);
        let used_lanes: std::collections::HashSet<usize> = g.rows.iter().map(|r| r.lane).collect();
        assert!(used_lanes.len() <= 2, "不应凭空多出泳道：{:?}", used_lanes);
    }

    #[test]
    fn empty_input_yields_single_lane() {
        let g = layout(&[]);
        assert_eq!(g.lane_count, 1);
        assert!(g.rows.is_empty());
    }

    #[test]
    fn row_count_matches_input() {
        let commits = vec![n("c", &["b"]), n("b", &["a"]), n("a", &[])];
        assert_eq!(layout(&commits).rows.len(), 3);
    }

    #[test]
    fn lane_indices_are_within_lane_count() {
        let commits = vec![
            n("m", &["A", "B"]),
            n("A", &["base"]),
            n("B", &["base"]),
            n("base", &[]),
        ];
        let g = layout(&commits);
        for row in &g.rows {
            assert!(row.lane < g.lane_count);
            for p in &row.parent_lanes {
                assert!(*p < g.lane_count, "父泳道越界：{p}");
            }
        }
    }
}
