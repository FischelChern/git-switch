//! 集成测试：在**真实**临时仓库上验证配置写入的隔离性、覆盖优先级与回滚。

use std::path::Path;
use std::process::Command;

use git_switch::git::config::{
    self, ConfigSnapshot, KEY_CREDENTIAL_HELPER, KEY_SSH_COMMAND, KEY_USER_EMAIL, KEY_USER_NAME,
};
use git_switch::model::identity::{AuthMode, Identity};
use git_switch::model::repository::{IdentityOverride, Repository};

/// 建一个真实的空 git 仓库。
fn init_repo(dir: &Path) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["init", "-q"])
        .output()
        .expect("无法执行 git，请确认已安装");
    assert!(out.status.success(), "git init 失败：{out:?}");
}

fn ssh_identity() -> Identity {
    Identity::new(
        "gh-me",
        "github",
        "张鑫宇 · 个人",
        "张鑫宇",
        "429092941@qq.com",
        AuthMode::Ssh,
    )
    .with_ssh_key("~/.ssh/id_ed25519_github")
}

fn token_identity() -> Identity {
    Identity::new(
        "corp-bot",
        "corp",
        "CI 机器人",
        "gitlab-ci",
        "ci@company.com",
        AuthMode::Token,
    )
}

fn project(id: &str, path: &Path) -> Repository {
    Repository::new(
        id,
        "github",
        path,
        "git@github.com:zhangxinyu/iot-gateway.git",
    )
}

#[test]
fn ssh_identity_writes_name_email_and_ssh_command() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    let id = ssh_identity();
    let repo = project("r1", dir.path());
    let effective = repo.effective_identity(&id);
    config::apply_identity(dir.path(), &id, &effective).expect("应能写入");

    let local = config::read_local(dir.path()).expect("应能读回");
    assert_eq!(local.user_name.as_deref(), Some("张鑫宇"));
    assert_eq!(local.user_email.as_deref(), Some("429092941@qq.com"));
    let ssh = local.ssh_command.expect("应写入 core.sshCommand");
    assert!(ssh.contains("~/.ssh/id_ed25519_github"), "实际：{ssh}");
    assert!(ssh.contains("IdentitiesOnly=yes"), "实际：{ssh}");
    assert_eq!(
        local.credential_helper, None,
        "SSH 身份不该设置 credential.helper"
    );
}

#[test]
fn token_identity_sets_credential_helper_and_clears_ssh() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    // 先写 SSH，再换成令牌，验证 sshCommand 被清掉
    let ssh_id = ssh_identity();
    let repo = project("r1", dir.path());
    config::apply_identity(dir.path(), &ssh_id, &repo.effective_identity(&ssh_id))
        .expect("应能写入 SSH");

    let token_id = token_identity();
    config::apply_identity(dir.path(), &token_id, &repo.effective_identity(&token_id))
        .expect("应能写入令牌");

    let local = config::read_local(dir.path()).expect("应能读回");
    assert_eq!(local.user_name.as_deref(), Some("gitlab-ci"));
    assert_eq!(local.user_email.as_deref(), Some("ci@company.com"));
    assert_eq!(local.ssh_command, None, "切换到令牌后应清掉 sshCommand");
    assert_eq!(
        local.credential_helper.as_deref(),
        Some(config::credential_helper()),
        "应写入平台默认凭据助手"
    );
}

#[test]
fn switching_one_repo_never_touches_another() {
    let a = tempfile::tempdir().expect("临时目录 A");
    let b = tempfile::tempdir().expect("临时目录 B");
    init_repo(a.path());
    init_repo(b.path());

    // B 先固定一套自己的身份
    let b_id = token_identity();
    let b_repo = project("rb", b.path());
    config::apply_identity(b.path(), &b_id, &b_repo.effective_identity(&b_id)).expect("写 B");
    let b_before = config::read_local(b.path()).expect("读 B");

    // 再切换 A
    let a_id = ssh_identity();
    let a_repo = project("ra", a.path());
    config::apply_identity(a.path(), &a_id, &a_repo.effective_identity(&a_id)).expect("写 A");

    let b_after = config::read_local(b.path()).expect("读 B");
    assert_eq!(b_before, b_after, "切换 A 不得影响 B 的本地配置");

    let a_after = config::read_local(a.path()).expect("读 A");
    assert_eq!(a_after.user_name.as_deref(), Some("张鑫宇"));
    assert_ne!(a_after.user_name, b_after.user_name, "两个仓库身份应不同");
}

#[test]
fn project_override_wins_over_identity() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    let id = ssh_identity();
    let mut repo = project("r1", dir.path());
    repo.identity_override = IdentityOverride::enabled_with(Some("阿张"), Some("zhang@corp.com"));

    let effective = repo.effective_identity(&id);
    config::apply_identity(dir.path(), &id, &effective).expect("应能写入");

    let local = config::read_local(dir.path()).expect("应能读回");
    assert_eq!(local.user_name.as_deref(), Some("阿张"), "应用项目覆盖值");
    assert_eq!(local.user_email.as_deref(), Some("zhang@corp.com"));
}

#[test]
fn override_only_email_keeps_identity_name_on_disk() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    let id = ssh_identity();
    let mut repo = project("r1", dir.path());
    repo.identity_override = IdentityOverride::enabled_with(None::<String>, Some("only@corp.com"));

    config::apply_identity(dir.path(), &id, &repo.effective_identity(&id)).expect("应能写入");

    let local = config::read_local(dir.path()).expect("应能读回");
    assert_eq!(local.user_name.as_deref(), Some("张鑫宇"), "姓名仍取身份值");
    assert_eq!(local.user_email.as_deref(), Some("only@corp.com"));
}

#[test]
fn clear_identity_removes_local_user_and_ssh() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    let id = ssh_identity();
    let repo = project("r1", dir.path());
    config::apply_identity(dir.path(), &id, &repo.effective_identity(&id)).expect("应能写入");
    assert!(
        config::read_local(dir.path())
            .expect("读")
            .user_name
            .is_some()
    );

    config::clear_identity(dir.path()).expect("应能清除");

    let local = config::read_local(dir.path()).expect("应能读回");
    assert_eq!(local.user_name, None);
    assert_eq!(local.user_email, None);
    assert_eq!(local.ssh_command, None);
}

#[test]
fn snapshot_restore_reverts_changes() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    config::set_local(dir.path(), KEY_USER_NAME, "原始名").expect("写");
    let snapshot = ConfigSnapshot::capture(dir.path()).expect("快照");

    config::set_local(dir.path(), KEY_USER_NAME, "被改的名字").expect("写");
    config::set_local(dir.path(), KEY_USER_EMAIL, "changed@example.com").expect("写");
    assert_eq!(
        config::get_local(dir.path(), KEY_USER_NAME)
            .expect("读")
            .as_deref(),
        Some("被改的名字")
    );

    snapshot.restore(dir.path()).expect("应能回滚");

    assert_eq!(
        config::get_local(dir.path(), KEY_USER_NAME)
            .expect("读")
            .as_deref(),
        Some("原始名"),
        "应恢复原值"
    );
    assert_eq!(
        config::get_local(dir.path(), KEY_USER_EMAIL).expect("读"),
        None,
        "原本不存在的键应被删除"
    );
}

#[test]
fn apply_to_missing_repo_fails_gracefully() {
    let id = ssh_identity();
    let effective = project("r1", Path::new("/nope")).effective_identity(&id);
    let err = config::apply_identity(Path::new("/definitely/not/a/repo"), &id, &effective)
        .expect_err("不存在的仓库应报错");
    let text = err.to_string();
    assert!(!text.is_empty(), "错误信息不应为空");
}

#[test]
fn missing_key_reads_as_none_and_unset_is_ok() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    assert_eq!(
        config::get_local(dir.path(), KEY_CREDENTIAL_HELPER).expect("读"),
        None
    );
    // 删除不存在的键不应报错
    config::unset_local(dir.path(), KEY_SSH_COMMAND).expect("删除不存在的键应成功");
}

#[test]
fn global_gitconfig_is_never_modified() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    // 把「全局」配置指向临时文件，避免污染真实 ~/.gitconfig
    let global = dir.path().join("fake-global-gitconfig");
    std::fs::write(&global, "[user]\n\tname = Global User\n").expect("写假全局配置");

    // SAFETY: 测试内设置进程环境变量，影响范围仅本进程
    unsafe {
        std::env::set_var("GIT_CONFIG_GLOBAL", &global);
    }
    let before = std::fs::read_to_string(&global).expect("读全局");

    let id = ssh_identity();
    let repo = project("r1", dir.path());
    config::apply_identity(dir.path(), &id, &repo.effective_identity(&id)).expect("应能写入");

    let after = std::fs::read_to_string(&global).expect("读全局");

    unsafe {
        std::env::remove_var("GIT_CONFIG_GLOBAL");
    }

    assert_eq!(before, after, "全局 gitconfig 必须一字未改");
}

#[test]
fn local_config_written_into_dot_git_config() {
    let dir = tempfile::tempdir().expect("临时目录");
    init_repo(dir.path());

    let id = ssh_identity();
    let repo = project("r1", dir.path());
    config::apply_identity(dir.path(), &id, &repo.effective_identity(&id)).expect("应能写入");

    let git_config = dir.path().join(".git/config");
    let text = std::fs::read_to_string(&git_config).expect("读 .git/config");
    assert!(text.contains("张鑫宇"), "应写进 .git/config：\n{text}");
    assert!(text.contains("429092941@qq.com"));
}
