// A model's answer, laid out from its text (round 45, D-086). The text is parsed into React
// nodes: headings, bold, numbered and bulleted lists, paragraphs. Other marks she asked not to
// see (single "*" or "_" emphasis, backticks, rule lines, table pipes) are dropped and their text
// kept. Nothing is ever turned into HTML: a "<" stays a "<". (CLAUDE.md: model output is text only.)
import "./RichText.css";

/** A run of text, plain or bold. */
export type Inline = { text: string; bold: boolean };
export type Block =
  | { kind: "heading"; level: 1 | 2 | 3; inline: Inline[] }
  | { kind: "paragraph"; inline: Inline[] }
  | { kind: "list"; ordered: boolean; start: number; items: Inline[][] };

/** Past this many lines the rest of the answer is one plain paragraph (a runaway answer stays readable). */
const MAX_LINES = 200;

/** "*a*", "_a_" and "`a`" → "a". Only a closed pair around a word is a mark: "5*3", a lone "*"
 *  and "file_name_x" stay as written. */
export function unmark(text: string): string {
  return text
    .replace(/`([^`\n]+)`/g, "$1")
    .replace(/(^|[\s(["'״׳])\*(?!\s)([^*\n]+?)(?<!\s)\*(?=$|[\s).,:;!?"'״׳])/g, "$1$2")
    .replace(/(^|[\s(["'״׳])_(?!\s)([^_\n]+?)(?<!\s)_(?=$|[\s).,:;!?"'״׳])/g, "$1$2");
}

/** "a **b** c" → a, b (bold), c. An unclosed "**" is ordinary text. */
export function inlines(text: string): Inline[] {
  const out: Inline[] = [];
  const re = /\*\*([^*\n]+?)\*\*/g;
  let last = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push({ text: unmark(text.slice(last, m.index)), bold: false });
    out.push({ text: unmark(m[1] ?? ""), bold: true });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: unmark(text.slice(last)), bold: false });
  return out;
}

/** "---", "***", "* * *", "___": a rule line, which reads as a break between paragraphs. */
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
/** "| a | b |": a table row. Its cells are read in order, joined by " · ". */
const TABLE_ROW = /^\s*\|(.*)\|\s*$/;
function tableCells(line: string): string[] | null {
  const m = TABLE_ROW.exec(line);
  if (!m) return null;
  return (m[1] ?? "").split("|").map((c) => c.trim());
}
/** "|---|:--:|": the line under a table's head, which carries nothing. */
const isTableRule = (cells: string[]) => cells.every((c) => /^:?-{2,}:?$/.test(c));

export function parseRich(text: string): Block[] {
  const all = text.replace(/\r\n?/g, "\n").split("\n");
  const lines = all.slice(0, MAX_LINES);
  const rest = all.slice(MAX_LINES).join(" ").trim();
  const blocks: Block[] = [];
  let para: string[] = [];
  const flush = () => {
    if (para.length) blocks.push({ kind: "paragraph", inline: inlines(para.join(" ")) });
    para = [];
  };
  for (const raw of lines) {
    const line = raw.trimEnd();
    if (!line.trim() || RULE.test(line)) { flush(); continue; }
    const cells = tableCells(line);
    if (cells) {
      flush();
      if (!isTableRule(cells)) blocks.push({ kind: "paragraph", inline: inlines(cells.filter(Boolean).join(" · ")) });
      continue;
    }
    const heading = /^(#{1,3})\s+(.+)$/.exec(line.trim());
    // A line that is nothing but "**…**" is how the model writes a heading too.
    const boldLine = /^\*\*([^*]+)\*\*:?$/.exec(line.trim());
    const ordered = /^\s*(\d{1,3})[.)]\s+(.+)$/.exec(line);
    const bullet = /^\s*[-•*]\s+(.+)$/.exec(line);
    if (heading) {
      flush();
      blocks.push({ kind: "heading", level: Math.min(3, heading[1]?.length ?? 1) as 1 | 2 | 3, inline: inlines(heading[2] ?? "") });
    } else if (boldLine) {
      flush();
      blocks.push({ kind: "heading", level: 3, inline: [{ text: unmark(boldLine[1] ?? ""), bold: false }] });
    } else if (ordered || bullet) {
      flush();
      const isOrdered = Boolean(ordered);
      const item = inlines((ordered ? ordered[2] : bullet?.[1]) ?? "");
      const prev = blocks[blocks.length - 1];
      if (prev?.kind === "list" && prev.ordered === isOrdered) prev.items.push(item);
      else blocks.push({ kind: "list", ordered: isOrdered, start: isOrdered ? Number(ordered?.[1] ?? 1) : 1, items: [item] });
    } else {
      para.push(line.trim());
    }
  }
  flush();
  if (rest) blocks.push({ kind: "paragraph", inline: [{ text: rest, bold: false }] });
  return blocks;
}

/** The same text without the marks, for the clipboard. */
export function plainText(text: string): string {
  return text
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .flatMap((l) => {
      if (RULE.test(l)) return [""];
      const cells = tableCells(l);
      if (cells) return isTableRule(cells) ? [] : [cells.filter(Boolean).join(" · ")];
      return [unmark(l.replace(/^(#{1,3})\s+/, "").replace(/\*\*([^*\n]+?)\*\*/g, "$1"))];
    })
    .join("\n");
}

function Runs({ inline }: { inline: Inline[] }) {
  return <>{inline.map((r, i) => (r.bold ? <strong key={i}>{r.text}</strong> : <span key={i}>{r.text}</span>))}</>;
}

/** The answer as it should read. Every node is text from the parser; no HTML is interpreted. */
export function RichText({ text, className }: { text: string; className?: string }) {
  const blocks = parseRich(text);
  return (
    <div className={className ? `rich ${className}` : "rich"}>
      {blocks.map((b, i) => {
        if (b.kind === "heading") {
          return b.level === 3 ? <h4 key={i}><Runs inline={b.inline} /></h4> : <h3 key={i}><Runs inline={b.inline} /></h3>;
        }
        if (b.kind === "list") {
          const items = b.items.map((it, j) => <li key={j}><Runs inline={it} /></li>);
          return b.ordered ? <ol key={i} start={b.start}>{items}</ol> : <ul key={i}>{items}</ul>;
        }
        return <p key={i}><Runs inline={b.inline} /></p>;
      })}
    </div>
  );
}
