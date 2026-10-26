import type { StyleItem } from "../../ipc/client";

/** The links of an inference's chain (D-100): `בתצפית: … ← מסקנה: … ← ניסוח: … ← המלצה: …`. */
export const CHAIN_SEP = " ← ";
export function chainLinks(text: string): { label: string; body: string }[] {
  return text.split(CHAIN_SEP).map((part) => {
    // The label ends at the first ": " outside parentheses: a score pattern's head is
    // `בתוצאות (המדד הנמוך ביותר: מהירות עיבוד): …`, and the colon inside stays with it.
    let depth = 0;
    let at = -1;
    for (let i = 0; i < part.length && i <= 80; i += 1) {
      const c = part[i];
      if (c === "(") depth += 1;
      else if (c === ")") depth = Math.max(0, depth - 1);
      else if (c === ":" && depth === 0 && part[i + 1] === " ") {
        at = i;
        break;
      }
    }
    return at > 0 ? { label: part.slice(0, at), body: part.slice(at + 2) } : { label: "", body: part };
  });
}

/** An inference as a chain, one link under the other, so the "why" reads as a thought. */
export function InferenceChain({ text, quiet }: { text: string; quiet?: boolean }) {
  const links = chainLinks(text);
  if (links.length < 2) return <>{text}</>;
  return (
    <span className={quiet ? "style-chain style-chain-quiet" : "style-chain"}>
      {links.map((l, i) => (
        <span key={i} className="style-chain-link">
          {l.label && <span className="style-chain-label">{l.label}</span>}
          <span className="style-chain-body">{l.body}</span>
        </span>
      ))}
    </span>
  );
}

/** An item's text as it reads best: a chain for an inference, plain text for the rest. */
export function ItemText({ item }: { item: StyleItem }) {
  return item.kind === "inference" ? <InferenceChain text={item.text} quiet /> : <>{item.text}</>;
}
