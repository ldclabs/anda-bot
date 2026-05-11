use ::markdown::{
    ParseOptions,
    mdast::{AlignKind, Node},
    unist::Position,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::{compact_cjk_spacing, display_width, normalize_newlines, theme};

#[derive(Clone, Copy)]
struct StyleRange {
    start: usize,
    end: usize,
    priority: u8,
    style: Style,
}

struct TableReplacement {
    start: usize,
    end: usize,
    lines: Vec<Line<'static>>,
}

struct LineBuilder {
    lines: Vec<Line<'static>>,
    current: Line<'static>,
}

impl LineBuilder {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current: Line::from(Vec::<Span<'static>>::new()),
        }
    }

    fn push_text(&mut self, text: &str, start_offset: usize, ranges: &[StyleRange]) {
        let mut local_start = 0;
        for line in text.split_inclusive('\n') {
            let has_newline = line.ends_with('\n');
            let line_text = if has_newline {
                &line[..line.len() - 1]
            } else {
                line
            };

            self.push_line_text(line_text, start_offset + local_start, ranges);
            local_start += line_text.len();

            if has_newline {
                self.finish_line();
                local_start += 1;
            }
        }
    }

    fn push_rendered_lines(&mut self, lines: Vec<Line<'static>>) {
        let mut lines = lines.into_iter();
        let Some(first) = lines.next() else {
            return;
        };

        if self.current.spans.is_empty() {
            self.current = first;
        } else {
            self.finish_line();
            self.current = first;
        }

        for line in lines {
            self.finish_line();
            self.current = line;
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.finish_line();
        self.lines
    }

    fn finish_line(&mut self) {
        let current = std::mem::replace(&mut self.current, Line::from(Vec::<Span<'static>>::new()));
        self.lines.push(current);
    }

    fn push_line_text(&mut self, text: &str, start_offset: usize, ranges: &[StyleRange]) {
        let end_offset = start_offset + text.len();
        let mut current_offset = start_offset;

        while current_offset < end_offset {
            let style = style_at(current_offset, ranges);
            let next_offset = next_style_boundary(current_offset, end_offset, ranges);
            let start = current_offset - start_offset;
            let end = next_offset - start_offset;
            push_span(&mut self.current, &text[start..end], style);
            current_offset = next_offset;
        }
    }
}

pub(super) fn render(text: &str) -> Vec<Line<'static>> {
    let text = normalize_newlines(text);
    let tree = match ::markdown::to_mdast(&text, &ParseOptions::gfm()) {
        Ok(tree) => tree,
        Err(err) => {
            log::warn!("Markdown parse failed in TUI transcript: {err}");
            return plain_text_lines(&text);
        }
    };

    let mut ranges = Vec::new();
    let mut tables = Vec::new();
    collect_node_metadata(&tree, &text, &mut ranges, &mut tables);
    ranges.sort_by_key(|range| (range.priority, range.start, range.end));
    tables.sort_by_key(|table| table.start);

    let mut builder = LineBuilder::new();
    let mut cursor = 0;
    for table in tables {
        if table.start < cursor || table.end > text.len() {
            continue;
        }

        builder.push_text(&text[cursor..table.start], cursor, &ranges);
        builder.push_rendered_lines(table.lines);
        cursor = table.end;
    }
    builder.push_text(&text[cursor..], cursor, &ranges);

    builder.finish()
}

fn collect_node_metadata(
    node: &Node,
    text: &str,
    ranges: &mut Vec<StyleRange>,
    tables: &mut Vec<TableReplacement>,
) {
    match node {
        Node::Root(root) => collect_children(&root.children, text, ranges, tables),
        Node::Heading(heading) => {
            push_node_style(node, ranges, heading_style(heading.depth), 10);
            collect_children(&heading.children, text, ranges, tables);
        }
        Node::Blockquote(blockquote) => {
            push_node_style(node, ranges, blockquote_style(), 10);
            collect_children(&blockquote.children, text, ranges, tables);
        }
        Node::List(list) => {
            push_node_style(node, ranges, list_style(), 10);
            collect_children(&list.children, text, ranges, tables);
        }
        Node::ListItem(item) => collect_children(&item.children, text, ranges, tables),
        Node::Table(table) => {
            if let Some(position) = node.position()
                && let Some(lines) = render_table(table, text, position)
            {
                tables.push(TableReplacement {
                    start: position.start.offset,
                    end: position.end.offset,
                    lines,
                });
            }
        }
        Node::Code(_) | Node::Math(_) | Node::Yaml(_) | Node::Toml(_) | Node::Html(_) => {
            push_node_style(node, ranges, code_block_style(), 10);
        }
        Node::MdxjsEsm(_) | Node::MdxFlowExpression(_) => {
            push_node_style(node, ranges, code_block_style(), 10);
        }
        Node::ThematicBreak(_) | Node::Definition(_) => {
            push_node_style(node, ranges, theme::dim_style(), 10);
        }
        Node::Paragraph(paragraph) => collect_children(&paragraph.children, text, ranges, tables),
        Node::Emphasis(emphasis) => {
            push_node_style(node, ranges, emphasis_style(), 30);
            collect_children(&emphasis.children, text, ranges, tables);
        }
        Node::Strong(strong) => {
            push_node_style(node, ranges, strong_style(), 31);
            collect_children(&strong.children, text, ranges, tables);
        }
        Node::Delete(delete) => {
            push_node_style(node, ranges, delete_style(), 30);
            collect_children(&delete.children, text, ranges, tables);
        }
        Node::InlineCode(_) | Node::InlineMath(_) => {
            push_node_style(node, ranges, inline_code_style(), 40);
        }
        Node::Link(link) => {
            push_node_style(node, ranges, link_style(), 40);
            collect_children(&link.children, text, ranges, tables);
        }
        Node::LinkReference(link) => {
            push_node_style(node, ranges, link_style(), 40);
            collect_children(&link.children, text, ranges, tables);
        }
        Node::Image(_) | Node::ImageReference(_) | Node::FootnoteReference(_) => {
            push_node_style(node, ranges, link_style(), 40);
        }
        Node::MdxJsxFlowElement(element) => {
            collect_children(&element.children, text, ranges, tables)
        }
        Node::MdxJsxTextElement(element) => {
            collect_children(&element.children, text, ranges, tables)
        }
        _ => {
            if let Some(children) = node.children() {
                collect_children(children, text, ranges, tables);
            }
        }
    }
}

fn collect_children(
    children: &[Node],
    text: &str,
    ranges: &mut Vec<StyleRange>,
    tables: &mut Vec<TableReplacement>,
) {
    for child in children {
        collect_node_metadata(child, text, ranges, tables);
    }
}

fn push_node_style(node: &Node, ranges: &mut Vec<StyleRange>, style: Style, priority: u8) {
    if let Some(position) = node.position() {
        push_style_range(ranges, position, style, priority);
    }
}

fn push_style_range(ranges: &mut Vec<StyleRange>, position: &Position, style: Style, priority: u8) {
    if position.start.offset < position.end.offset {
        ranges.push(StyleRange {
            start: position.start.offset,
            end: position.end.offset,
            priority,
            style,
        });
    }
}

fn style_at(offset: usize, ranges: &[StyleRange]) -> Style {
    ranges
        .iter()
        .filter(|range| range.start <= offset && offset < range.end)
        .fold(Style::default(), |style, range| style.patch(range.style))
}

fn next_style_boundary(offset: usize, end: usize, ranges: &[StyleRange]) -> usize {
    ranges
        .iter()
        .filter_map(|range| {
            if range.start > offset && range.start < end {
                Some(range.start)
            } else if range.end > offset && range.end < end {
                Some(range.end)
            } else {
                None
            }
        })
        .min()
        .unwrap_or(end)
}

fn render_table(
    table: &::markdown::mdast::Table,
    text: &str,
    position: &Position,
) -> Option<Vec<Line<'static>>> {
    let rows = table_source_rows(text, position).unwrap_or_else(|| table_ast_rows(table));

    if rows.is_empty() {
        return None;
    }

    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths = table_column_widths(&rows, &table.align, column_count);
    let mut lines = Vec::with_capacity(rows.len() + 1);

    for (idx, row) in rows.iter().enumerate() {
        let style = if idx == 0 {
            table_header_style()
        } else {
            table_body_style()
        };
        lines.push(line_from_text(
            &render_table_row(row, &widths, &table.align),
            style,
        ));

        if idx == 0 {
            lines.push(line_from_text(
                &render_table_separator(&widths, &table.align),
                table_separator_style(),
            ));
        }
    }

    Some(lines)
}

fn table_source_rows(text: &str, position: &Position) -> Option<Vec<Vec<String>>> {
    let source = text.get(position.start.offset..position.end.offset)?;
    let mut lines = source.lines();
    let header = split_table_row(lines.next()?)?;
    lines.next()?;

    let mut rows = vec![header];
    rows.extend(lines.filter_map(split_table_row));
    Some(rows)
}

fn table_ast_rows(table: &::markdown::mdast::Table) -> Vec<Vec<String>> {
    table
        .children
        .iter()
        .filter_map(|row| match row {
            Node::TableRow(row) => Some(
                row.children
                    .iter()
                    .map(|cell| normalize_table_cell_text(&cell.to_string()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .collect()
}

fn split_table_row(line: &str) -> Option<Vec<String>> {
    let mut row = line.trim();
    row = row.strip_prefix('|').unwrap_or(row);
    row = row.strip_suffix('|').unwrap_or(row);

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for ch in row.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }

        if ch == '|' {
            cells.push(normalize_table_cell_text(&current));
            current.clear();
        } else {
            current.push(ch);
        }
    }

    cells.push(normalize_table_cell_text(&current));

    if cells.is_empty() { None } else { Some(cells) }
}

fn normalize_table_cell_text(text: &str) -> String {
    compact_cjk_spacing(text.trim().replace('\n', " ").as_ref()).into_owned()
}

fn table_column_widths(
    rows: &[Vec<String>],
    alignments: &[AlignKind],
    column_count: usize,
) -> Vec<usize> {
    (0..column_count)
        .map(|idx| {
            let cell_width = rows
                .iter()
                .filter_map(|row| row.get(idx))
                .map(|cell| display_width(cell))
                .max()
                .unwrap_or(0);
            cell_width.max(separator_min_width(
                alignments.get(idx).copied().unwrap_or(AlignKind::None),
            ))
        })
        .collect()
}

fn separator_min_width(align: AlignKind) -> usize {
    match align {
        AlignKind::Center => 5,
        AlignKind::Left | AlignKind::Right => 4,
        AlignKind::None => 3,
    }
}

fn render_table_row(row: &[String], widths: &[usize], alignments: &[AlignKind]) -> String {
    let cells = widths
        .iter()
        .enumerate()
        .map(|(idx, width)| {
            align_cell(
                row.get(idx).map(String::as_str).unwrap_or_default(),
                *width,
                alignments.get(idx).copied().unwrap_or(AlignKind::None),
            )
        })
        .collect::<Vec<_>>();
    format!("| {} |", cells.join(" | "))
}

fn render_table_separator(widths: &[usize], alignments: &[AlignKind]) -> String {
    let cells = widths
        .iter()
        .enumerate()
        .map(|(idx, width)| {
            separator_cell(
                *width,
                alignments.get(idx).copied().unwrap_or(AlignKind::None),
            )
        })
        .collect::<Vec<_>>();
    format!("| {} |", cells.join(" | "))
}

fn align_cell(text: &str, width: usize, align: AlignKind) -> String {
    let padding = width.saturating_sub(display_width(text));
    match align {
        AlignKind::Right => format!("{}{}", " ".repeat(padding), text),
        AlignKind::Center => {
            let left = padding / 2;
            let right = padding - left;
            format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
        }
        AlignKind::Left | AlignKind::None => format!("{}{}", text, " ".repeat(padding)),
    }
}

fn separator_cell(width: usize, align: AlignKind) -> String {
    let width = width.max(separator_min_width(align));
    match align {
        AlignKind::Left => format!(":{}", "-".repeat(width - 1)),
        AlignKind::Right => format!("{}:", "-".repeat(width - 1)),
        AlignKind::Center => format!(":{}:", "-".repeat(width - 2)),
        AlignKind::None => "-".repeat(width),
    }
}

fn push_span(line: &mut Line<'static>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }

    let text = compact_cjk_spacing(text);
    if text.is_empty() {
        return;
    }

    if let Some(last) = line.spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(text.as_ref());
        return;
    }
    line.spans.push(Span::styled(text.into_owned(), style));
}

fn plain_text_lines(text: &str) -> Vec<Line<'static>> {
    text.split('\n')
        .map(|line| line_from_text(compact_cjk_spacing(line).as_ref(), Style::default()))
        .collect()
}

fn line_from_text(text: &str, style: Style) -> Line<'static> {
    Line::from(vec![Span::styled(text.to_string(), style)])
}

fn heading_style(level: u8) -> Style {
    match level {
        1 => Style::default()
            .fg(theme::PANDA_WHITE)
            .add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(theme::BAMBOO_LIGHT)
            .add_modifier(Modifier::BOLD),
        3 => Style::default()
            .fg(theme::BAMBOO_GREEN)
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(theme::LEAF_MINT)
            .add_modifier(Modifier::BOLD),
    }
}

fn blockquote_style() -> Style {
    Style::default()
        .fg(theme::BAMBOO_DIM)
        .add_modifier(Modifier::ITALIC)
}

fn list_style() -> Style {
    Style::default().fg(theme::BAMBOO_LIGHT)
}

fn code_block_style() -> Style {
    Style::default().fg(theme::ACCENT_TEAL).bg(theme::FOOTER_BG)
}

fn inline_code_style() -> Style {
    Style::default().fg(theme::ACCENT_TEAL).bg(theme::FOOTER_BG)
}

fn link_style() -> Style {
    Style::default()
        .fg(theme::ACCENT_TEAL)
        .add_modifier(Modifier::UNDERLINED)
}

fn emphasis_style() -> Style {
    Style::default().add_modifier(Modifier::ITALIC)
}

fn strong_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn delete_style() -> Style {
    Style::default()
        .fg(theme::BAMBOO_DIM)
        .add_modifier(Modifier::CROSSED_OUT)
}

fn table_header_style() -> Style {
    Style::default()
        .fg(theme::LEAF_MINT)
        .add_modifier(Modifier::BOLD)
}

fn table_separator_style() -> Style {
    Style::default().fg(theme::BAMBOO_DIM)
}

fn table_body_style() -> Style {
    Style::default().fg(theme::BAMBOO_LIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn headings_preserve_source_and_add_style() {
        let lines = render("# Title\n\n### Detail");

        assert_eq!(line_text(&lines[0]), "# Title");
        assert_eq!(line_text(&lines[1]), "");
        assert_eq!(line_text(&lines[2]), "### Detail");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(lines[0].spans[0].style.fg, Some(theme::PANDA_WHITE));
    }

    #[test]
    fn inline_markdown_preserves_source_and_adds_styles() {
        let lines = render("Hello **bold** and `code`.");

        assert_eq!(line_text(&lines[0]), "Hello **bold** and `code`.");
        let bold = lines[0]
            .spans
            .iter()
            .find(|span| span.content.contains("**bold**"))
            .expect("bold span");
        let code = lines[0]
            .spans
            .iter()
            .find(|span| span.content.contains("`code`"))
            .expect("code span");

        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(code.style.fg, Some(theme::ACCENT_TEAL));
        assert_eq!(code.style.bg, Some(theme::FOOTER_BG));
    }

    #[test]
    fn tables_render_as_aligned_markdown_source() {
        let lines = render("| Name | Count |\n| :--- | ---: |\n| alpha | 2 |\n| beta | 10 |");

        assert_eq!(line_text(&lines[0]), "| Name  | Count |");
        assert_eq!(line_text(&lines[1]), "| :---- | ----: |");
        assert_eq!(line_text(&lines[2]), "| alpha |     2 |");
        assert_eq!(line_text(&lines[3]), "| beta  |    10 |");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(lines[1].spans[0].style.fg, Some(theme::BAMBOO_DIM));
    }
}
