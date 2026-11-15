//! 交互式变基对话框的测试：从日志发起、调整顺序和处理方式、按键、开始变基和截图。

use winc_editor::git::RebaseAction;

use crate::app::git::form::FormKind;
use crate::app::input::context_menu::MenuCommand;
use crate::layout::DialogPart;

use super::*;

/// 打开有 `a`、`b`、`c`、`d` 四个提交的仓库，从 `b` 发起交互式变基。
fn rebasing(name: &str) -> Option<(App, PathBuf)> {
    let dir = search_folder(name);
    let run = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let mut ready = run(&["init", "-q", "-b", "main"])
        && run(&["config", "user.name", "t"])
        && run(&["config", "user.email", "t@t"])
        && run(&["config", "commit.gpgsign", "false"]);
    for name in ["a", "b", "c", "d"] {
        std::fs::write(dir.join(format!("{name}.txt")), name).unwrap();
        ready = ready && run(&["add", "--all"]) && run(&["commit", "-q", "-m", name]);
    }
    if !ready {
        skip_without_git("无法用 git 建测试仓库");
        return None;
    }
    let mut app = app_with(&dir, false);
    app.settings_toggle(Setting::GitModule);
    if git_unusable(&app) {
        return None;
    }
    let oid = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "HEAD~2"])
        .output()
        .unwrap();
    let oid = String::from_utf8(oid.stdout).unwrap().trim().to_owned();
    app.start_interactive_rebase(oid);
    paint(&mut app);
    Some((app, dir))
}

fn form(app: &App) -> &crate::app::git::rebase::RebaseForm {
    match &app.overlays.form_dialog.as_ref().unwrap().kind {
        FormKind::Rebase(form) => form,
        _ => panic!("应当是交互式变基对话框"),
    }
}

fn subjects(dir: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    text.lines().map(str::to_owned).collect()
}

fn key(app: &mut App, name: NamedKey) {
    assert!(app.dialog_key(&Key::Named(name), "", false));
}

fn letter(app: &mut App, c: &str) {
    assert!(app.dialog_key(&Key::Character(c.into()), c, false));
}

#[test]
fn the_dialog_lists_the_range_oldest_first_and_starts_unchanged() {
    let Some((app, _dir)) = rebasing("rebase-dialog") else {
        return;
    };
    let names: Vec<&str> = (form(&app).rows.iter())
        .map(|row| row.commit.subject.as_str())
        .collect();
    assert_eq!(names, ["b", "c", "d"]);
    // 没有更改时不能开始。
    let dialog = app.overlays.form_dialog.as_ref().unwrap();
    assert!(!app.form_plan(dialog).enabled);
}

#[test]
fn keys_select_move_and_set_actions() {
    let Some((mut app, dir)) = rebasing("rebase-keys") else {
        return;
    };
    // 第一个提交不能压缩。
    letter(&mut app, "s");
    let dialog = app.overlays.form_dialog.as_ref().unwrap();
    assert!(app.form_plan(dialog).problem.is_some());
    letter(&mut app, "p");

    key(&mut app, NamedKey::ArrowDown);
    assert_eq!(form(&app).selected, 1);
    letter(&mut app, "f");
    assert_eq!(form(&app).rows[1].action, RebaseAction::Fixup);
    key(&mut app, NamedKey::ArrowDown);
    letter(&mut app, "d");
    assert_eq!(form(&app).rows[2].action, RebaseAction::Drop);

    key(&mut app, NamedKey::Enter);
    assert!(app.overlays.form_dialog.is_none());
    assert_eq!(subjects(&dir), ["b", "a"]);
    assert!(dir.join("c.txt").exists() && !dir.join("d.txt").exists());
}

#[test]
fn rewording_edits_the_message_and_enter_adds_a_line() {
    let Some((mut app, dir)) = rebasing("rebase-reword") else {
        return;
    };
    letter(&mut app, "r");
    let dialog = app.overlays.form_dialog.as_ref().unwrap();
    assert_eq!(dialog.focus, DialogPart::Branch);
    assert_eq!(dialog.name.text(), "b");
    let field = &mut app.overlays.form_dialog.as_mut().unwrap().name;
    field.set_caret(field.text().len(), false);
    paint(&mut app);
    app.overlays
        .form_dialog
        .as_mut()
        .unwrap()
        .name
        .insert(" new");
    assert!(app.dialog_key(&Key::Named(NamedKey::Enter), "", false));
    assert_eq!(
        app.overlays.form_dialog.as_ref().unwrap().name.text(),
        "b new\n"
    );
    app.overlays
        .form_dialog
        .as_mut()
        .unwrap()
        .name
        .insert("body");

    // ⌘回车开始变基。
    assert!(app.dialog_key(&Key::Named(NamedKey::Enter), "", true));
    assert!(app.overlays.form_dialog.is_none());
    let message = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["log", "-1", "--format=%B", "HEAD~2"])
        .output()
        .unwrap();
    let message = String::from_utf8(message.stdout).unwrap();
    assert_eq!(message.trim_end(), "b new\nbody");
}

#[test]
fn moving_a_row_keeps_it_selected_and_changes_the_order() {
    let Some((mut app, dir)) = rebasing("rebase-move") else {
        return;
    };
    app.press_rebase_part(DialogPart::RebaseDown);
    assert_eq!(form(&app).selected, 1);
    assert_eq!(form(&app).rows[1].commit.subject, "b");
    // 已在末尾和开头时不移动。
    app.press_rebase_part(DialogPart::RebaseDown);
    app.press_rebase_part(DialogPart::RebaseDown);
    assert_eq!(form(&app).rows[2].commit.subject, "b");
    app.submit_form_dialog();
    assert_eq!(subjects(&dir), ["b", "d", "c", "a"]);
}

#[test]
fn the_action_menu_sets_the_row_it_opened_on() {
    let Some((mut app, _dir)) = rebasing("rebase-menu") else {
        return;
    };
    app.press_rebase_part(DialogPart::RebaseAction(2));
    app.run_menu_command(MenuCommand::RebaseAction(RebaseAction::Edit));
    assert_eq!(form(&app).rows[2].action, RebaseAction::Edit);
    assert_eq!(form(&app).selected, 2);
}

#[test]
#[ignore = "截图，手动运行"]
fn snapshot_rebase_dialog() {
    let Some((mut app, _dir)) = rebasing("rebase-shot") else {
        return;
    };
    letter(&mut app, "p");
    key(&mut app, NamedKey::ArrowDown);
    letter(&mut app, "d");
    key(&mut app, NamedKey::ArrowDown);
    letter(&mut app, "f");
    std::thread::sleep(std::time::Duration::from_millis(400));
    shoot(&mut app, "git-rebase");
}

#[test]
fn edit_message_opens_with_full_text_and_preserves_descendants() {
    let Some((mut app, dir)) = rebasing("edit-commit-message") else {
        return;
    };
    let commits = form(&app)
        .rows
        .iter()
        .map(|row| row.commit.clone())
        .collect();
    let base = form(&app).base.clone();
    app.close_form_dialog();
    app.open_reword_dialog(winc_editor::git::RebasePlan {
        base,
        commits,
        pushed: 0,
    });
    let dialog = app.overlays.form_dialog.as_ref().unwrap();
    assert_eq!(dialog.focus, DialogPart::Branch);
    assert_eq!(dialog.name.text(), "b");
    assert_eq!(form(&app).rows[0].action, RebaseAction::Reword);
    assert!(
        form(&app).rows[1..]
            .iter()
            .all(|row| row.action == RebaseAction::Pick)
    );
    app.overlays
        .form_dialog
        .as_mut()
        .unwrap()
        .name
        .set_text("edited subject\n\nbody");
    assert!(app.dialog_key(&Key::Named(NamedKey::Enter), "", true));
    assert_eq!(subjects(&dir), ["d", "c", "edited subject", "a"]);
    for name in ["a", "b", "c", "d"] {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{name}.txt"))).unwrap(),
            name
        );
    }
}

#[test]
fn squash_selected_commits_preserves_the_tree_and_other_commits() {
    let Some((mut app, dir)) = rebasing("squash-selected") else {
        return;
    };
    let commits: Vec<_> = form(&app)
        .rows
        .iter()
        .map(|row| row.commit.clone())
        .collect();
    let oids = vec![commits[0].oid.clone(), commits[2].oid.clone()];
    let base = form(&app).base.clone();
    app.close_form_dialog();
    app.open_squash_dialog(
        winc_editor::git::RebasePlan {
            base,
            commits,
            pushed: 0,
        },
        &oids,
    );
    let dialog = app.overlays.form_dialog.as_ref().unwrap();
    assert_eq!(dialog.name.text(), "b\n\nd");
    assert_eq!(dialog.focus, DialogPart::Branch);
    app.overlays
        .form_dialog
        .as_mut()
        .unwrap()
        .name
        .set_text("combined\n\nbody");
    assert!(app.dialog_key(&Key::Named(NamedKey::Enter), "", true));
    assert_eq!(subjects(&dir), ["c", "combined", "a"]);
    for name in ["a", "b", "c", "d"] {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{name}.txt"))).unwrap(),
            name
        );
    }
}

#[test]
fn editing_a_long_commit_message_starts_at_the_top_and_keeps_the_full_message() {
    let Some((mut app, dir)) = rebasing("reword-long-message") else {
        return;
    };
    let message = format!(
        "完整提交标题\n\n{}\nCode Source From: Self Code\n\nAI Co-author: GPT-6.1-sol",
        "正文内容与验证结果\n".repeat(20)
    );
    let mut commits: Vec<_> = form(&app)
        .rows
        .iter()
        .map(|row| row.commit.clone())
        .collect();
    commits[0].message = message.clone();
    let base = form(&app).base.clone();
    app.close_form_dialog();
    app.open_reword_dialog(winc_editor::git::RebasePlan {
        base,
        commits,
        pushed: 0,
    });
    paint(&mut app);
    let field = &app.overlays.form_dialog.as_ref().unwrap().name;
    assert_eq!(field.text(), message);
    assert_eq!(field.caret(), 0);
    assert_eq!(field.view.scroll, 0.0);
    let area = field.view.area;
    let point = (area.x + area.w / 2.0, area.y + area.h / 2.0);
    let target = app.scroll_target_at(point.0, point.1);
    assert_eq!(target, crate::app::scroll::ScrollTarget::DialogMessage);
    app.scroll_target_by(target, point, false, (0.0, -80.0));
    paint(&mut app);
    let field = &app.overlays.form_dialog.as_ref().unwrap().name;
    assert_eq!(field.view.scroll, 80.0);
    assert_eq!(field.caret(), 0);
    assert_eq!(field.text(), message);
    assert_eq!(form(&app).scroll, 0.0);
    // 多次重绘不把滚轮滚动的位置拉回光标。
    paint(&mut app);
    assert_eq!(
        app.overlays.form_dialog.as_ref().unwrap().name.view.scroll,
        80.0
    );
    app.scroll_target_by(target, point, false, (0.0, -10000.0));
    paint(&mut app);
    let field = &app.overlays.form_dialog.as_ref().unwrap().name;
    assert_eq!(field.view.scroll, field.view.vertical_scroll_max());
    app.scroll_target_by(target, point, false, (0.0, 10000.0));
    paint(&mut app);
    assert_eq!(
        app.overlays.form_dialog.as_ref().unwrap().name.view.scroll,
        0.0
    );
    let layout = app.dialog_layout();
    let bar_x = layout.branch.right() - 5.0;
    app.click_form_dialog(bar_x, area.y + 1.0);
    assert!(matches!(
        app.pointer.drag,
        crate::app::Drag::DialogMessageScrollbar { .. }
    ));
    app.hover_form_dialog(bar_x, area.bottom() + 100.0);
    paint(&mut app);
    let field = &app.overlays.form_dialog.as_ref().unwrap().name;
    assert_eq!(field.view.scroll, field.view.vertical_scroll_max());
    assert_eq!(field.caret(), 0);
    app.pointer.drag = crate::app::Drag::None;
    assert_eq!(
        app.scroll_target_at(layout.list.x + 2.0, layout.list.y + 2.0),
        crate::app::scroll::ScrollTarget::Dialog
    );
    assert_eq!(
        app.scroll_target_at(0.0, 0.0),
        crate::app::scroll::ScrollTarget::Blocked
    );
    let field = &mut app.overlays.form_dialog.as_mut().unwrap().name;
    field.set_caret(field.text().len(), false);
    paint(&mut app);
    assert!(app.overlays.form_dialog.as_ref().unwrap().name.view.scroll > 0.0);
    // 切走再切回，不能继承前一次的底部滚动位置。
    app.set_rebase_action(1, RebaseAction::Reword);
    app.set_rebase_action(0, RebaseAction::Reword);
    paint(&mut app);
    let field = &app.overlays.form_dialog.as_ref().unwrap().name;
    assert_eq!(field.text(), message);
    assert_eq!(field.view.scroll, 0.0);
    assert!(app.dialog_key(&Key::Named(NamedKey::Enter), "", true));
    let output = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["log", "-1", "--format=%B", "HEAD~2"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim_end(),
        message.trim_end()
    );
}
