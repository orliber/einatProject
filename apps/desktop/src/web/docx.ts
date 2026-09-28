// Browser preview only: reads the text of a .docx (body, headers/footers, author) so the import
// screen can be tried without the app. The real reader is crates/dv-ingest, in an isolated process.

async function unzip(bytes: Uint8Array): Promise<Map<string, Uint8Array>> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let eocd = -1;
  for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 65_557); i--) {
    if (view.getUint32(i, true) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error("not a zip");
  const count = view.getUint16(eocd + 10, true);
  let at = view.getUint32(eocd + 16, true);
  const out = new Map<string, Uint8Array>();
  for (let n = 0; n < count; n++) {
    if (view.getUint32(at, true) !== 0x02014b50) break;
    const method = view.getUint16(at + 10, true);
    const size = view.getUint32(at + 20, true);
    const nameLen = view.getUint16(at + 28, true);
    const extraLen = view.getUint16(at + 30, true);
    const commentLen = view.getUint16(at + 32, true);
    const local = view.getUint32(at + 42, true);
    const name = new TextDecoder().decode(bytes.subarray(at + 46, at + 46 + nameLen));
    at += 46 + nameLen + extraLen + commentLen;
    if (!/^(word\/(document|header\d*|footer\d*)\.xml|docProps\/core\.xml)$/.test(name)) continue;
    const start = local + 30 + view.getUint16(local + 26, true) + view.getUint16(local + 28, true);
    const data = bytes.subarray(start, start + size);
    if (method === 0) {
      out.set(name, data);
    } else if (method === 8) {
      const stream = new Blob([new Uint8Array(data)]).stream().pipeThrough(new DecompressionStream("deflate-raw"));
      out.set(name, new Uint8Array(await new Response(stream).arrayBuffer()));
    }
  }
  return out;
}

const W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

function lines(xml: string): string[] {
  const doc = new DOMParser().parseFromString(xml, "application/xml");
  const out: string[] = [];
  const text = (el: Element) => Array.from(el.getElementsByTagNameNS(W, "t")).map((t) => t.textContent ?? "").join("");
  const body = doc.documentElement;
  const walk = (el: Element) => {
    for (const child of Array.from(el.children)) {
      if (child.localName === "p") out.push(text(child));
      else if (child.localName === "tbl") {
        for (const row of Array.from(child.getElementsByTagNameNS(W, "tr"))) {
          out.push(Array.from(row.getElementsByTagNameNS(W, "tc")).map(text).join("\t"));
        }
      } else walk(child);
    }
  };
  walk(body);
  return out.map((l) => l.trim()).filter(Boolean);
}

export interface DocxText {
  body: string;
  margins: string[];
  author: string | null;
}

export async function readDocx(bytes: Uint8Array): Promise<DocxText> {
  const files = await unzip(bytes);
  const decode = (name: string) => new TextDecoder().decode(files.get(name));
  if (!files.has("word/document.xml")) throw new Error("no body");
  const margins = Array.from(files.keys())
    .filter((n) => /header|footer/.test(n))
    .flatMap((n) => lines(decode(n)));
  let author: string | null = null;
  if (files.has("docProps/core.xml")) {
    const core = new DOMParser().parseFromString(decode("docProps/core.xml"), "application/xml");
    author = core.getElementsByTagNameNS("http://purl.org/dc/elements/1.1/", "creator")[0]?.textContent?.trim() || null;
  }
  return { body: lines(decode("word/document.xml")).join("\n"), margins, author };
}
