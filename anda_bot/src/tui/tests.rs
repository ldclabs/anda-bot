use std::path::PathBuf;

use anda_core::{ContentPart, Message};
use anda_engine::memory::{Conversation, ConversationStatus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
    text::{Line, Span},
};
use tokio::sync::oneshot;

use crate::{auto_update::AutoUpdateState, config::Config, gateway};

use super::{
    INPUT_DIVIDER_LABEL, INPUT_DIVIDER_PADDED_HEIGHT, INPUT_DIVIDER_PREFIX,
    SECONDARY_PART_MAX_LINES, THINKING_FRAMES, THINKING_LABEL,
    app::App,
    input::{
        InputCursorDirection, build_input_viewport, build_prompt_lines, input_display_text,
        input_height, input_newline_key, input_placeholder, input_prompt_prefix_width,
        input_scroll_top, input_separator_line, input_separator_lines, move_cursor_vertically,
        split_input_area,
    },
    layout::{
        centered_area, dynamic_layout_heights, dynamic_viewport_height, static_panel_height,
        status_footer_height, status_footer_panel, status_panel_height,
    },
    render::render,
    status::{panel_header_line, panel_lines, status_footer_lines, status_line},
    terminal::cleanup_inline_viewport,
    text::{display_width, normalize_newlines, truncate_visual, wrap_visual},
    theme,
    transcript::{chat_message_lines, chat_message_lines_for_message, thinking_lines},
    widgets::PackedLines,
};

fn test_client() -> gateway::Client {
    gateway::Client::new("http://127.0.0.1:8042".to_string(), String::new())
}

fn ready_app() -> App {
    let mut app = App::new(PathBuf::from("."), Config::default(), test_client());
    app.daemon_running = true;
    app
}

fn push_text_message(app: &mut App, role: &str, text: &str) {
    app.chat.messages.push(anda_core::Message {
        role: role.to_string(),
        content: vec![ContentPart::Text {
            text: text.to_string(),
        }],
        ..Default::default()
    });
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn clear_message_view_requests_terminal_purge() {
    let mut app = ready_app();
    app.static_panel_flushed = true;
    app.flushed_message_count = 7;
    app.input_focused = false;

    app.clear_message_view();

    assert_eq!(app.flushed_message_count, 0);
    assert!(app.input_focused);
    assert!(!app.static_panel_flushed);
    assert!(app.pending_scrollback_purge);
}

#[test]
fn normalize_newlines_converts_crlf_and_cr() {
    assert_eq!(normalize_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
}

#[test]
fn insert_input_text_respects_cursor_position() {
    let mut app = ready_app();
    app.input_buf = "abef".to_string();
    app.input_cursor = 2;

    app.insert_input_text("cd");

    assert_eq!(app.input_buf, "abcdef");
    assert_eq!(app.input_cursor, 4);
}

#[test]
fn insert_input_text_compacts_cjk_spaces_but_keeps_english_spaces() {
    let mut app = ready_app();

    app.insert_input_text("你 好 hello world 再 见");

    assert_eq!(app.input_buf, "你好 hello world 再见");
    assert_eq!(app.input_cursor, "你好 hello world 再见".chars().count());
}

#[test]
fn handle_paste_inserts_normalized_multiline_text() {
    let mut app = ready_app();

    app.handle_paste("first\r\nsecond\rthird".to_string());

    assert_eq!(app.input_buf, "first\nsecond\nthird");
    assert_eq!(app.input_cursor, "first\nsecond\nthird".chars().count());
}

#[test]
fn input_newline_key_accepts_shift_enter_and_ctrl_j() {
    assert!(input_newline_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
    )));
    assert!(input_newline_key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    )));
    assert!(!input_newline_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
}

#[test]
fn move_cursor_vertically_preserves_visual_column() {
    let text = "one\ntwo\nthree";

    let (cursor, preferred_col) =
        move_cursor_vertically(text, 2, 20, InputCursorDirection::Down, None);
    assert_eq!(cursor, 6);
    assert_eq!(preferred_col, 2);

    let (cursor, preferred_col) = move_cursor_vertically(
        text,
        cursor,
        20,
        InputCursorDirection::Down,
        Some(preferred_col),
    );
    assert_eq!(cursor, 10);
    assert_eq!(preferred_col, 2);

    let (cursor, _) = move_cursor_vertically(
        text,
        cursor,
        20,
        InputCursorDirection::Up,
        Some(preferred_col),
    );
    assert_eq!(cursor, 6);
}

#[test]
fn move_cursor_vertically_handles_wrapped_lines() {
    let (cursor, preferred_col) =
        move_cursor_vertically("abcdef", 6, 3, InputCursorDirection::Up, None);

    assert_eq!(cursor, 3);
    assert_eq!(preferred_col, 0);
}

#[test]
fn build_prompt_lines_uses_continuation_prefix_for_multiline_input() {
    let mut app = ready_app();
    app.input_buf = "alpha\nbeta".to_string();
    app.input_cursor = app.input_buf.chars().count();

    let lines = build_prompt_lines(&app, "", 24);

    assert_eq!(lines.len(), 2);
    assert_eq!(line_text(&lines[0]), "❯ alpha");
    assert_eq!(line_text(&lines[1]), "  beta");
}

#[test]
fn build_prompt_lines_hides_placeholder_while_input_is_focused() {
    let app = ready_app();

    let lines = build_prompt_lines(&app, input_placeholder(&app), 24);

    assert_eq!(lines.len(), 1);
    assert_eq!(line_text(&lines[0]), "❯ ");
}

#[test]
fn input_viewport_follows_cursor_to_bottom_of_long_paste() {
    let mut app = ready_app();
    app.input_buf = "one\ntwo\nthree\nfour\nfive\nsix".to_string();
    app.input_cursor = app.input_buf.chars().count();

    let viewport = build_input_viewport(&app, "", 24, 4);
    let visible: Vec<_> = viewport.lines.iter().map(line_text).collect();

    assert_eq!(viewport.total_lines, 6);
    assert_eq!(viewport.scroll_top, 2);
    assert_eq!(visible, vec!["  three", "  four", "  five", "  six"]);
}

#[test]
fn input_viewport_keeps_cursor_line_visible_when_moved_up() {
    let mut app = ready_app();
    app.input_buf = "one\ntwo\nthree\nfour\nfive\nsix".to_string();
    app.input_cursor = 0;

    let viewport = build_input_viewport(&app, "", 24, 4);
    let visible: Vec<_> = viewport.lines.iter().map(line_text).collect();

    assert_eq!(viewport.scroll_top, 0);
    assert_eq!(visible, vec!["❯ one", "  two", "  three", "  four"]);
}

#[test]
fn input_viewport_adds_virtual_line_when_cursor_wraps_past_full_row() {
    let mut app = ready_app();
    app.input_buf = "abcd".to_string();
    app.input_cursor = app.input_buf.chars().count();

    let viewport = build_input_viewport(&app, "", input_prompt_prefix_width() + 4, 4);
    let visible: Vec<_> = viewport.lines.iter().map(line_text).collect();

    assert_eq!(viewport.cursor_row, 1);
    assert_eq!(viewport.total_lines, 2);
    assert_eq!(visible, vec!["❯ abcd", "  "]);
}

#[test]
fn input_scroll_top_tracks_cursor_without_exceeding_content() {
    assert_eq!(input_scroll_top(0, 4, 6), 0);
    assert_eq!(input_scroll_top(3, 4, 6), 0);
    assert_eq!(input_scroll_top(4, 4, 6), 1);
    assert_eq!(input_scroll_top(9, 4, 6), 2);
    assert_eq!(input_scroll_top(9, 0, 6), 0);
}

#[test]
fn input_height_reserves_space_for_separator() {
    let app = ready_app();

    assert_eq!(
        input_height(
            &app,
            Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 8,
            }
        ),
        4
    );
}

#[test]
fn cleanup_inline_viewport_clears_from_viewport_top_and_moves_below_it() {
    let mut output = Vec::new();

    cleanup_inline_viewport(
        &mut output,
        Rect {
            x: 2,
            y: 4,
            width: 40,
            height: 3,
        },
    )
    .expect("cleanup succeeds");

    assert_eq!(
        String::from_utf8(output).expect("valid ansi"),
        "\u{1b}[5;3H\u{1b}[J\u{1b}[3E"
    );
}

#[test]
fn split_input_area_preserves_prompt_when_only_one_line_fits() {
    let area = Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 1,
    };

    let (separator, prompt) = split_input_area(area);

    assert!(separator.is_none());
    assert_eq!(prompt, area);
}

#[test]
fn split_input_area_reserves_padding_when_room_allows() {
    let area = Rect {
        x: 2,
        y: 3,
        width: 40,
        height: 6,
    };

    let (separator, prompt) = split_input_area(area);

    assert_eq!(separator.unwrap().height, INPUT_DIVIDER_PADDED_HEIGHT);
    assert_eq!(prompt.y, area.y + INPUT_DIVIDER_PADDED_HEIGHT);
    assert_eq!(prompt.height, area.height - INPUT_DIVIDER_PADDED_HEIGHT);
}

#[test]
fn input_separator_lines_add_blank_padding_when_room_allows() {
    let app = ready_app();
    let lines = input_separator_lines(
        &app,
        Rect {
            x: 0,
            y: 0,
            width: 24,
            height: INPUT_DIVIDER_PADDED_HEIGHT,
        },
    );

    assert_eq!(line_text(&lines[0]), "");
    assert!(line_text(&lines[1]).starts_with("── compose "));
    assert_eq!(line_text(&lines[2]), "");
}

#[test]
fn input_separator_line_labels_compose_section() {
    let app = ready_app();
    let line = input_separator_line(&app, 24);

    assert!(line_text(&line).starts_with("── compose "));
}

#[test]
fn input_separator_line_shows_fixed_width_thinking_status() {
    let mut app = ready_app();
    app.chat.conversation = Some(Conversation {
        status: ConversationStatus::Submitted,
        ..Default::default()
    });

    app.animation_tick = 0;
    let first = line_text(&input_separator_line(&app, 32));
    app.animation_tick = 2;
    let second = line_text(&input_separator_line(&app, 32));
    let frame_widths: Vec<_> = THINKING_FRAMES
        .iter()
        .map(|frame| display_width(&format!("{THINKING_LABEL} {frame}")))
        .collect();

    assert!(first.starts_with("── thinking ⠋ "));
    assert!(second.starts_with("── thinking ⠙ "));
    assert_ne!(first, second);
    assert_eq!(display_width(&first), 32);
    assert_eq!(display_width(&second), 32);
    assert!(
        THINKING_FRAMES
            .iter()
            .all(|frame| display_width(frame) == 1)
    );
    assert!(frame_widths.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn input_separator_title_has_no_background() {
    let app = ready_app();
    let line = input_separator_line(&app, 24);
    let label_start = INPUT_DIVIDER_PREFIX.chars().count();
    let label_end = label_start + INPUT_DIVIDER_LABEL.chars().count();

    assert!(
        line.spans[label_start..label_end]
            .iter()
            .all(|span| span.style.bg.is_none())
    );
    assert!(
        line.spans[..label_start]
            .iter()
            .any(|span| matches!(span.style.bg, Some(Color::Rgb(..))))
    );
}

#[test]
fn input_separator_line_animates_truecolor_glow() {
    let mut app = ready_app();
    app.animation_tick = 0;
    let before = input_separator_line(&app, 24);

    app.animation_tick = 8;
    let after = input_separator_line(&app, 24);

    let before_colors: Vec<_> = before.spans.iter().map(|span| span.style.fg).collect();
    let after_colors: Vec<_> = after.spans.iter().map(|span| span.style.fg).collect();
    let before_backgrounds: Vec<_> = before.spans.iter().map(|span| span.style.bg).collect();
    let after_backgrounds: Vec<_> = after.spans.iter().map(|span| span.style.bg).collect();

    assert_eq!(line_text(&before), line_text(&after));
    assert!(
        before_colors
            .iter()
            .any(|color| matches!(color, Some(Color::Rgb(..))))
    );
    assert!(
        before_backgrounds
            .iter()
            .any(|color| matches!(color, Some(Color::Rgb(..))))
    );
    assert_ne!(before_colors, after_colors);
    assert_ne!(before_backgrounds, after_backgrounds);
}

#[test]
fn chat_message_lines_render_thoughts_as_limited_dim_excerpt() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "assistant".to_string(),
        content: vec![
            ContentPart::Text {
                text: "Answer ready".to_string(),
            },
            ContentPart::Reasoning {
                text: "first line of reasoning\nsecond line that is long enough to truncate"
                    .to_string(),
            },
        ],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 28);

    assert_eq!(line_text(&lines[0]), "🐼 ❯ Answer ready");
    assert_eq!(line_text(&lines[1]), "");
    let secondary = &lines[2..lines.len() - 1];
    assert!(secondary.len() <= SECONDARY_PART_MAX_LINES);
    assert_eq!(secondary[0].spans[0].style, theme::dim_style());
    assert_eq!(secondary[0].spans[1].style, theme::dim_style());
    assert!(line_text(&secondary[0]).contains("thinking:"));
    assert!(line_text(secondary.last().expect("secondary line")).ends_with("..."));
    assert_eq!(line_text(lines.last().expect("blank line")), "");
}

#[test]
fn chat_message_lines_insert_gap_before_limited_parts() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "assistant".to_string(),
        content: vec![
            ContentPart::Text {
                text: "正文内容".to_string(),
            },
            ContentPart::ToolCall {
                name: "search".to_string(),
                args: serde_json::json!({"q": "anda"}),
                call_id: None,
            },
        ],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 80);

    assert_eq!(line_text(&lines[0]), "🐼 ❯ 正文内容");
    assert_eq!(line_text(&lines[1]), "");
    assert!(line_text(&lines[2]).starts_with("     → search("));
}

#[test]
fn chat_message_lines_expand_multiline_text() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "assistant".to_string(),
        content: vec![ContentPart::Text {
            text: "line one\nline two\nline three".to_string(),
        }],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 40);

    assert_eq!(line_text(&lines[0]), "🐼 ❯ line one");
    assert_eq!(line_text(&lines[1]), "     line two");
    assert_eq!(line_text(&lines[2]), "     line three");
    assert_eq!(line_text(&lines[3]), "");
}

#[test]
fn chat_message_lines_render_markdown_source_styles() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "assistant".to_string(),
        content: vec![ContentPart::Text {
            text: "## Title\n\nHello **bold** and `code`.".to_string(),
        }],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 80);

    assert_eq!(line_text(&lines[0]), "🐼 ❯ ## Title");
    assert!(
        lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(lines[0].spans[1].style.fg, Some(theme::BAMBOO_LIGHT));

    let paragraph = lines
        .iter()
        .find(|line| line_text(line).contains("Hello"))
        .expect("paragraph line");
    let bold = paragraph
        .spans
        .iter()
        .find(|span| span.content.contains("**bold**"))
        .expect("bold span");
    let code = paragraph
        .spans
        .iter()
        .find(|span| span.content.contains("`code`"))
        .expect("code span");

    assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(line_text(paragraph), "     Hello **bold** and `code`.");
    assert_eq!(code.style.fg, Some(theme::ACCENT_TEAL));
    assert_eq!(code.style.bg, Some(theme::FOOTER_BG));
    assert_eq!(line_text(lines.last().expect("blank line")), "");
}

#[test]
fn chat_message_lines_render_markdown_tables() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "assistant".to_string(),
        content: vec![ContentPart::Text {
            text: "| Name | Count |\n| :--- | ---: |\n| alpha | 2 |\n| beta | 10 |".to_string(),
        }],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 80);

    assert_eq!(line_text(&lines[0]), "🐼 ❯ | Name  | Count |");
    assert_eq!(line_text(&lines[1]), "     | :---- | ----: |");
    assert_eq!(line_text(&lines[2]), "     | alpha |     2 |");
    assert_eq!(line_text(&lines[3]), "     | beta  |    10 |");
    assert_eq!(line_text(lines.last().expect("blank line")), "");
}

#[test]
fn chat_message_lines_render_tool_calls_as_dim_summary() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "assistant".to_string(),
        content: vec![ContentPart::ToolCall {
            name: "search".to_string(),
            args: serde_json::json!({"q": "anda"}),
            call_id: None,
        }],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 80);

    assert!(line_text(&lines[0]).starts_with("🐼 ❯ → search("));
    assert_eq!(lines[0].spans[1].style, theme::dim_style());
}

#[test]
fn chat_message_lines_limit_tool_output_to_three_lines() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "tool".to_string(),
        content: vec![ContentPart::ToolOutput {
            name: "shell".to_string(),
            output: serde_json::json!({"stdout": "a very long output line that should wrap and be truncated before it can take over the transcript area"}),
            is_error: None,
            call_id: None,
            remote_id: None,
        }],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 28);
    let secondary = &lines[..lines.len() - 1];

    assert_eq!(secondary.len(), SECONDARY_PART_MAX_LINES);
    assert!(
        secondary
            .iter()
            .all(|line| line.spans[1].style == theme::dim_style())
    );
    assert!(line_text(secondary.last().expect("secondary line")).ends_with("..."));
}

#[test]
fn chat_message_lines_render_errors_with_system_prefix() {
    let mut app = ready_app();
    app.chat.messages.push(anda_core::Message {
        role: "system".to_string(),
        content: vec![ContentPart::Text {
            text: "request failed badly".to_string(),
        }],
        ..Default::default()
    });

    let lines = chat_message_lines(&app, 40);

    assert_eq!(line_text(&lines[0]), "⚠️ ❯ request failed badly");
    assert_eq!(lines[0].spans[0].style, theme::danger_style());
    assert_eq!(lines[0].spans[1].style, theme::danger_style());
    assert_eq!(line_text(&lines[1]), "");
}

#[test]
fn chat_message_lines_keep_cjk_text_contiguous() {
    let mut app = ready_app();
    push_text_message(&mut app, "user", "前面出错了，再试试。");

    let lines = chat_message_lines(&app, 80);

    assert_eq!(line_text(&lines[0]), "❯ 前面出错了，再试试。");
    assert_eq!(display_width("前面出错了，再试试。"), 20);
}

#[test]
fn chat_message_lines_compact_ascii_spaces_between_cjk() {
    let mut app = ready_app();
    push_text_message(&mut app, "assistant", "已 提 交，依 赖 版 本 升 级。");

    let lines = chat_message_lines(&app, 80);

    assert_eq!(line_text(&lines[0]), "🐼 ❯ 已提交，依赖版本升级。");
}

#[test]
fn wrap_visual_splits_cjk_by_display_width_without_spaces() {
    assert_eq!(wrap_visual("前面出错", 4), vec!["前面", "出错"]);
    assert_eq!(truncate_visual("前面出错", 7), "前面...");
}

#[test]
fn dynamic_viewport_height_excludes_static_messages() {
    let mut app = ready_app();
    let before = dynamic_viewport_height(&app, 80, 30);
    for idx in 0..6 {
        push_text_message(&mut app, "user", &format!("message {idx}"));
    }

    assert_eq!(dynamic_viewport_height(&app, 80, 30), before);
}

#[test]
fn status_footer_switches_between_help_and_status() {
    let mut app = ready_app();

    let help = status_footer_lines(&app, 80);
    assert!(line_text(&help[0]).starts_with("? "));

    app.input_focused = false;
    let status = status_footer_lines(&app, 80);
    assert!(line_text(&status[0]).starts_with("READY "));
}

#[test]
fn status_footer_height_reserves_separator_row() {
    let app = ready_app();

    assert_eq!(status_footer_height(&app, 80), 3);
}

#[test]
fn status_footer_panel_starts_below_divider() {
    assert_eq!(
        status_footer_panel(Rect {
            x: 2,
            y: 4,
            width: 30,
            height: 3,
        }),
        Rect {
            x: 2,
            y: 5,
            width: 30,
            height: 2,
        }
    );
}

#[test]
fn status_footer_notice_compacts_spaced_cjk() {
    let mut app = ready_app();
    app.notice = "连 接 失 败，请 重 试。".to_string();

    let lines = status_footer_lines(&app, 80);

    assert_eq!(
        line_text(lines.last().expect("notice line")),
        "! 连接失败，请重试。"
    );
}

#[test]
fn thinking_lines_only_render_for_submitted_and_working() {
    let mut app = ready_app();

    assert!(thinking_lines(&app).is_empty());

    app.chat.conversation = Some(Conversation {
        status: ConversationStatus::Idle,
        ..Default::default()
    });
    assert!(thinking_lines(&app).is_empty());

    app.chat.conversation = Some(Conversation {
        status: ConversationStatus::Submitted,
        ..Default::default()
    });
    assert_eq!(line_text(&thinking_lines(&app)[0]), "🐼 ❯ thinking ⠋");

    app.chat.conversation = Some(Conversation {
        status: ConversationStatus::Working,
        ..Default::default()
    });
    assert_eq!(line_text(&thinking_lines(&app)[0]), "🐼 ❯ thinking ⠋");

    app.chat.conversation = Some(Conversation {
        status: ConversationStatus::Completed,
        ..Default::default()
    });
    assert!(thinking_lines(&app).is_empty());
}

#[test]
fn packed_lines_writes_each_grapheme_to_its_own_cell() {
    use ratatui::widgets::Widget;

    let area = Rect {
        x: 0,
        y: 0,
        width: 20,
        height: 1,
    };
    let mut buf = Buffer::empty(area);
    PackedLines::new(vec![Line::from(vec![
        Span::styled("❯ ", theme::accent_style()),
        Span::styled("中文", theme::body_style()),
    ])])
    .render(area, &mut buf);

    // Prefix grapheme-by-grapheme: "❯" at col 0, " " at col 1.
    assert_eq!(buf[(0, 0)].symbol(), "❯");
    assert_eq!(buf[(1, 0)].symbol(), " ");
    // CJK graphemes occupy width-2 cells: cell at col 2 holds "中"
    // and ratatui implicitly skips col 3; "文" lands at col 4.
    assert_eq!(buf[(2, 0)].symbol(), "中");
    assert_eq!(buf[(4, 0)].symbol(), "文");
    // Style preserved on the body cells.
    assert_eq!(buf[(2, 0)].fg, theme::body_style().fg.unwrap());
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

#[test]
fn app_accessors_report_paths_and_state() {
    let app = ready_app();
    assert_eq!(app.config_file_path(), PathBuf::from("./config.yaml"));
    assert!(app.log_file_path().to_string_lossy().contains("logs"));
    assert!(!app.setup_required());
    assert!(app.chat_enabled());
    let daemon = app.runtime_daemon();
    assert_eq!(daemon.home, PathBuf::from("."));
}

#[test]
fn insert_input_text_tracks_cursor() {
    let mut app = ready_app();
    app.insert_input_text("hello");
    assert_eq!(app.input_buf, "hello");
    assert_eq!(app.input_cursor, 5);
    app.input_cursor = 0;
    app.insert_input_text("X");
    assert_eq!(app.input_buf, "Xhello");
    // Empty insert is a no-op.
    app.insert_input_text("");
    assert_eq!(app.input_buf, "Xhello");
}

#[test]
fn handle_paste_inserts_only_when_enabled() {
    let mut app = ready_app();
    app.handle_paste("pasted\r\ntext".to_string());
    assert!(app.input_buf.contains("pasted"));

    // While sending, paste is ignored.
    let mut busy = ready_app();
    busy.chat.sending = true;
    busy.handle_paste("ignored".to_string());
    assert!(busy.input_buf.is_empty());
}

#[test]
fn apply_update_state_sets_notice_once() {
    let mut app = ready_app();
    let state = AutoUpdateState {
        status: crate::auto_update::AutoUpdateStatus::Downloaded,
        latest_tag: Some("v9.9.9".to_string()),
        downloaded_path: Some("/tmp/anda".to_string()),
        ..Default::default()
    };
    assert!(app.apply_update_state(state.clone()));
    assert!(app.notice.contains("v9.9.9"));
    // Applying the same notice again is a no-op.
    assert!(!app.apply_update_state(state));

    // A state without a CLI notice does not set anything.
    let mut clean = ready_app();
    assert!(!clean.apply_update_state(AutoUpdateState::default()));
}

#[test]
fn finish_pending_update_check_handles_closed_and_empty_channels() {
    let mut app = ready_app();
    // No pending check.
    assert!(!app.finish_pending_update_check());

    // A dropped sender closes the channel.
    let (tx, rx) = oneshot::channel::<Result<AutoUpdateState, String>>();
    drop(tx);
    app.pending_update_check = Some(rx);
    assert!(!app.finish_pending_update_check());
    assert!(app.pending_update_check.is_none());

    // A delivered Ok update applies its notice.
    let (tx, rx) = oneshot::channel();
    tx.send(Ok(AutoUpdateState {
        status: crate::auto_update::AutoUpdateStatus::Downloaded,
        latest_tag: Some("v8.0.0".to_string()),
        downloaded_path: Some("/tmp/anda".to_string()),
        ..Default::default()
    }))
    .unwrap();
    app.pending_update_check = Some(rx);
    assert!(app.finish_pending_update_check());
    assert!(app.notice.contains("v8.0.0"));
}

#[tokio::test]
async fn handle_key_edits_input_buffer() {
    let mut app = ready_app();
    let w = 40;

    app.handle_key(key(KeyCode::Char('a')), w).await.unwrap();
    app.handle_key(key(KeyCode::Char('b')), w).await.unwrap();
    app.handle_key(key(KeyCode::Char('c')), w).await.unwrap();
    assert_eq!(app.input_buf, "abc");

    app.handle_key(key(KeyCode::Left), w).await.unwrap();
    assert_eq!(app.input_cursor, 2);
    app.handle_key(key(KeyCode::Backspace), w).await.unwrap();
    assert_eq!(app.input_buf, "ac");
    app.handle_key(key(KeyCode::Delete), w).await.unwrap();
    assert_eq!(app.input_buf, "a");

    app.handle_key(key(KeyCode::Right), w).await.unwrap();
    app.handle_key(key(KeyCode::Home), w).await.unwrap();
    assert_eq!(app.input_cursor, 0);
    app.handle_key(key(KeyCode::End), w).await.unwrap();
    assert_eq!(app.input_cursor, 1);

    // Up/Down cursor movement does not panic.
    app.handle_key(key(KeyCode::Up), w).await.unwrap();
    app.handle_key(key(KeyCode::Down), w).await.unwrap();

    // Shift+Enter inserts a newline.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), w)
        .await
        .unwrap();
    assert!(app.input_buf.contains('\n'));
}

#[tokio::test]
async fn handle_key_control_shortcuts() {
    let mut app = ready_app();
    app.insert_input_text("hello world");

    app.handle_key(ctrl(KeyCode::Char('a')), 40).await.unwrap();
    assert_eq!(app.input_cursor, 0);
    app.handle_key(ctrl(KeyCode::Char('e')), 40).await.unwrap();
    assert_eq!(app.input_cursor, app.input_buf.chars().count());
    app.handle_key(ctrl(KeyCode::Char('u')), 40).await.unwrap();
    assert!(app.input_buf.is_empty());

    app.handle_key(ctrl(KeyCode::Char('c')), 40).await.unwrap();
    assert!(app.should_quit);
}

#[tokio::test]
async fn handle_key_escape_toggles_input_focus() {
    let mut app = ready_app();
    app.notice = "something".to_string();
    app.handle_key(key(KeyCode::Esc), 40).await.unwrap();
    assert!(app.notice.is_empty());
    assert!(!app.input_focused);

    // With input unfocused, a non-esc key refocuses.
    app.handle_key(key(KeyCode::Char('x')), 40).await.unwrap();
    assert!(app.input_focused);
}

#[tokio::test]
async fn handle_key_ignored_while_sending() {
    let mut app = ready_app();
    app.chat.sending = true;
    app.handle_key(key(KeyCode::Char('z')), 40).await.unwrap();
    assert!(app.input_buf.is_empty());
}

#[tokio::test]
async fn submit_input_handles_empty_and_text() {
    let mut app = ready_app();
    // Empty input does nothing.
    app.submit_input().await.unwrap();
    assert!(!app.chat.sending);

    app.insert_input_text("hello there");
    app.submit_input().await.unwrap();
    // The input buffer is cleared after submission.
    assert!(app.input_buf.is_empty());
}

#[tokio::test]
async fn bootstrap_reports_setup_or_daemon_state() {
    let home = tempfile::tempdir().unwrap();
    let mut app = App::new(home.path().to_path_buf(), Config::default(), test_client());
    app.bootstrap().await;
    // Either a setup notice or a daemon connection notice is produced.
    assert!(!app.notice.is_empty());
}

#[test]
fn render_draws_full_frame_without_panicking() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut app = ready_app();
    push_text_message(&mut app, "user", "hello");
    push_text_message(&mut app, "assistant", "hi there");
    app.notice = "a notice".to_string();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &mut app)).unwrap();

    // Render again after focusing out to exercise alternate layout.
    app.input_focused = false;
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
}

#[test]
fn render_draws_setup_screen_when_not_ready() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut app = App::new(PathBuf::from("."), Config::default(), test_client());
    app.daemon_running = false;
    app.setup.issues = vec!["model.active".to_string()];
    app.notice = "fill in config".to_string();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
}

fn diverse_message(role: &str) -> Message {
    Message {
        role: role.to_string(),
        content: vec![
            ContentPart::Text {
                text: "# Heading\n\nbody text with `code` and a long line that should wrap across the available width nicely".to_string(),
            },
            ContentPart::Reasoning {
                text: "thinking about it".to_string(),
            },
            ContentPart::ToolCall {
                name: "shell".to_string(),
                args: serde_json::json!({"command": "ls"}),
                call_id: Some("c1".to_string()),
            },
            ContentPart::ToolOutput {
                name: "shell".to_string(),
                output: serde_json::json!({"stdout": "ok"}),
                call_id: Some("c1".to_string()),
                is_error: None,
                remote_id: None,
            },
            ContentPart::FileData {
                file_uri: "file:///tmp/a.png".to_string(),
                mime_type: Some("image/png".to_string()),
            },
            ContentPart::InlineData {
                mime_type: "image/png".to_string(),
                data: anda_core::ByteBufB64(vec![1, 2, 3]),
            },
        ],
        ..Default::default()
    }
}

#[test]
fn chat_message_lines_render_all_roles_and_parts() {
    for role in ["user", "assistant", "system", "tool", "other"] {
        let lines = chat_message_lines_for_message(&diverse_message(role), 60);
        assert!(!lines.is_empty(), "role {role} produced no lines");
    }

    let mut app = ready_app();
    app.chat.messages = vec![diverse_message("user"), diverse_message("assistant")];
    assert!(!chat_message_lines(&app, 60).is_empty());
}

#[test]
fn render_helpers_cover_state_variants() {
    let area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };

    // Ready app with content and a notice.
    let mut ready = ready_app();
    push_text_message(&mut ready, "user", "hi");
    ready.notice = "a notice".to_string();
    ready.input_buf = "draft input".to_string();
    ready.input_cursor = 3;

    assert!(!status_footer_lines(&ready, 80).is_empty());
    let _ = status_line(&ready, 80);
    let _ = input_display_text(&ready);
    let _ = input_placeholder(&ready);
    let _ = build_prompt_lines(&ready, input_placeholder(&ready), 80);
    let _ = panel_lines(&ready);
    let _ = panel_header_line(&ready);
    let _ = dynamic_layout_heights(&ready, area);
    let _ = dynamic_viewport_height(&ready, 80, 24);
    let _ = status_panel_height(&ready, area);
    let _ = static_panel_height(&ready, 80);
    let _ = status_footer_height(&ready, 80);
    let _ = input_height(&ready, area);
    let _ = thinking_lines(&ready);

    // Not-ready app (setup required) exercises the alternate branches.
    let mut setup = App::new(PathBuf::from("."), Config::default(), test_client());
    setup.daemon_running = false;
    setup.setup.issues = vec!["model.active".to_string()];
    let _ = status_line(&setup, 80);
    let _ = input_placeholder(&setup);
    let _ = status_footer_lines(&setup, 80);

    // Daemon down (ready setup, not running).
    let mut down = ready_app();
    down.daemon_running = false;
    let _ = status_line(&down, 80);
    let _ = input_placeholder(&down);
}

#[test]
fn centered_area_constrains_width() {
    let area = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 10,
    };
    let centered = centered_area(area, 40);
    assert!(centered.width <= 40);
    assert!(centered.x >= area.x);
    // A max wider than the area keeps the full width.
    assert_eq!(centered_area(area, 500).width, 100);
}
