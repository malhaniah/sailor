// A small Markdown renderer for release notes: headings, paragraphs, bullet lists,
// rules, **bold**, `code` and https links. Everything is escaped first, so a release
// body can never put markup of its own on the page.

export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function inline(text: string): string {
  // code spans are left exactly as written; formatting applies between them
  return text
    .split(/(`[^`]+`)/)
    .map((part, i) => {
      if (i % 2 === 1) return `<code>${escapeHtml(part.slice(1, -1))}</code>`;
      return escapeHtml(part)
        .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
        .replace(
          /\[([^\]]+)\]\((https:\/\/[^\s)]+)\)/g,
          '<a href="$2" rel="noopener">$1</a>',
        );
    })
    .join("");
}

export interface MarkdownOptions {
  /** "## x" becomes <h(2 + headingShift)> */
  headingShift?: number;
}

export function renderMarkdown(md: string, opts: MarkdownOptions = {}): string {
  const shift = opts.headingShift ?? 0;
  const out: string[] = [];
  let para: string[] = [];
  let list: string[] | null = null;

  const flush = () => {
    if (para.length) out.push(`<p>${inline(para.join(" "))}</p>`);
    para = [];
    if (list) out.push(`<ul>${list.map((li) => `<li>${inline(li)}</li>`).join("")}</ul>`);
    list = null;
  };

  for (const raw of md.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trim();
    if (!line) {
      flush();
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      const level = Math.min(6, heading[1].length + shift);
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      continue;
    }
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(line)) {
      flush();
      out.push("<hr>");
      continue;
    }
    const item = /^[-*+]\s+(.*)$/.exec(line);
    if (item) {
      if (para.length) {
        out.push(`<p>${inline(para.join(" "))}</p>`);
        para = [];
      }
      (list ??= []).push(item[1]);
      continue;
    }
    if (list) {
      // a continuation line of the last item
      list[list.length - 1] += ` ${line}`;
      continue;
    }
    para.push(line);
  }
  flush();
  return out.join("\n");
}
