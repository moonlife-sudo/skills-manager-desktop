import { useMemo } from "react";
import { structuredPatch } from "diff";
import type { StructuredPatchHunk } from "diff";
import { cn } from "../utils";

interface DocumentDiffViewerProps {
  original: string;
  updated: string;
  className?: string;
}

type DiffRow =
  | { type: "context"; leftNumber: number; rightNumber: number; leftContent: string; rightContent: string }
  | { type: "removed"; leftNumber: number; rightNumber: null; leftContent: string; rightContent: "" }
  | { type: "added"; leftNumber: null; rightNumber: number; leftContent: ""; rightContent: string }
  | { type: "changed"; leftNumber: number; rightNumber: number; leftContent: string; rightContent: string };

interface RenderHunk {
  id: string;
  leftStart: number;
  leftCount: number;
  rightStart: number;
  rightCount: number;
  rows: DiffRow[];
}

const CONTEXT_LINES = 3;

function normalizeLineEndings(value: string) {
  return value.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

function normalizeNoNewlineSentinel(content: string) {
  return content === "\\ No newline at end of file" ? "" : content;
}

function flushPendingRemoved(rows: DiffRow[], pendingRemoved: Array<{ number: number; content: string }>) {
  while (pendingRemoved.length > 0) {
    const removed = pendingRemoved.shift();
    if (!removed) break;
    rows.push({
      type: "removed",
      leftNumber: removed.number,
      rightNumber: null,
      leftContent: removed.content,
      rightContent: "",
    });
  }
}

function rowsFromHunk(hunk: StructuredPatchHunk): DiffRow[] {
  const rows: DiffRow[] = [];
  const pendingRemoved: Array<{ number: number; content: string }> = [];
  let leftNumber = hunk.oldStart;
  let rightNumber = hunk.newStart;

  for (const rawLine of hunk.lines) {
    if (!rawLine) continue;
    const marker = rawLine[0];
    const content = normalizeNoNewlineSentinel(rawLine.slice(1));

    if (marker === "-") {
      pendingRemoved.push({ number: leftNumber, content });
      leftNumber += 1;
      continue;
    }

    if (marker === "+") {
      const removed = pendingRemoved.shift();
      if (removed) {
        rows.push({
          type: "changed",
          leftNumber: removed.number,
          rightNumber,
          leftContent: removed.content,
          rightContent: content,
        });
      } else {
        rows.push({
          type: "added",
          leftNumber: null,
          rightNumber,
          leftContent: "",
          rightContent: content,
        });
      }
      rightNumber += 1;
      continue;
    }

    flushPendingRemoved(rows, pendingRemoved);
    rows.push({
      type: "context",
      leftNumber,
      rightNumber,
      leftContent: content,
      rightContent: content,
    });
    leftNumber += 1;
    rightNumber += 1;
  }

  flushPendingRemoved(rows, pendingRemoved);
  return rows;
}

function buildHunks(original: string, updated: string): RenderHunk[] {
  const normalizedOriginal = normalizeLineEndings(original);
  const normalizedUpdated = normalizeLineEndings(updated);
  if (normalizedOriginal === normalizedUpdated) return [];
  const patch = structuredPatch("original", "updated", normalizedOriginal, normalizedUpdated, "", "", {
    context: CONTEXT_LINES,
  });

  return patch.hunks.map((hunk, index) => ({
    id: `hunk-${index}-${hunk.oldStart}-${hunk.newStart}`,
    leftStart: hunk.oldStart,
    leftCount: hunk.oldLines,
    rightStart: hunk.newStart,
    rightCount: hunk.newLines,
    rows: rowsFromHunk(hunk),
  }));
}

function cellTone(type: DiffRow["type"], side: "left" | "right") {
  if ((type === "removed" || type === "changed") && side === "left") {
    return {
      lineNoClass: "text-red-900 dark:text-red-200",
      lineNoStyle: { backgroundColor: "#ffd7d5" },
      codeClass: "text-red-950 dark:text-red-50",
      codeStyle: { backgroundColor: "#ffebe9", boxShadow: "inset 3px 0 0 #cf222e" },
      markerClass: "text-red-700 dark:text-red-300",
    };
  }
  if ((type === "added" || type === "changed") && side === "right") {
    return {
      lineNoClass: "text-emerald-900 dark:text-emerald-200",
      lineNoStyle: { backgroundColor: "#aceebb" },
      codeClass: "text-emerald-950 dark:text-emerald-50",
      codeStyle: { backgroundColor: "#dafbe1", boxShadow: "inset 3px 0 0 #1a7f37" },
      markerClass: "text-emerald-700 dark:text-emerald-300",
    };
  }
  return {
    lineNoClass: "text-faint",
    lineNoStyle: { backgroundColor: "var(--color-surface-hover)" },
    codeClass: "text-secondary",
    codeStyle: { backgroundColor: "var(--color-bg-secondary)" },
    markerClass: "text-faint",
  };
}

function markerFor(type: DiffRow["type"], side: "left" | "right") {
  if (side === "left") return type === "removed" || type === "changed" ? "-" : " ";
  return type === "added" || type === "changed" ? "+" : " ";
}

function DiffCell({
  number,
  content,
  type,
  side,
}: {
  number: number | null;
  content: string;
  type: DiffRow["type"];
  side: "left" | "right";
}) {
  const tone = cellTone(type, side);

  return (
    <>
      <td
        className={cn("w-14 select-none border-r border-border-subtle px-3 text-right font-mono text-[12px]", tone.lineNoClass)}
        style={tone.lineNoStyle}
      >
        {number ?? ""}
      </td>
      <td
        className={cn("border-r border-border-subtle px-3 font-mono text-[12.5px] leading-6", tone.codeClass)}
        style={tone.codeStyle}
      >
        <span className={cn("mr-3 inline-block w-3 select-none text-center font-semibold", tone.markerClass)}>
          {markerFor(type, side)}
        </span>
        <span className="whitespace-pre-wrap break-words">{content || " "}</span>
      </td>
    </>
  );
}

export function DocumentDiffViewer({ original, updated, className }: DocumentDiffViewerProps) {
  const hunks = useMemo(() => buildHunks(original, updated), [original, updated]);

  if (hunks.length === 0) {
    return (
      <div className={cn("rounded-xl border border-border-subtle bg-bg-secondary px-4 py-6 text-center", className)}>
        <div className="text-[13px] font-medium text-secondary">No content changes</div>
      </div>
    );
  }

  return (
    <div className={cn("space-y-4", className)}>
      {hunks.map((hunk) => (
        <div key={hunk.id} className="overflow-hidden rounded-xl border border-border-subtle bg-bg-secondary">
          <div className="grid grid-cols-2 border-b border-border-subtle" style={{ backgroundColor: "#ddf4ff" }}>
            <div className="border-r border-border-subtle px-3 py-2 font-mono text-[11px] text-sky-800">
              @@ -{hunk.leftStart},{hunk.leftCount}
            </div>
            <div className="px-3 py-2 font-mono text-[11px] text-sky-800">
              @@ +{hunk.rightStart},{hunk.rightCount}
            </div>
          </div>

          <div className="overflow-x-auto">
            <table className="min-w-full border-collapse">
              <tbody>
                {hunk.rows.map((row, index) => (
                  <tr key={`${hunk.id}-${index}`} className="border-b border-border-subtle/80 last:border-b-0">
                    <DiffCell
                      number={row.leftNumber}
                      content={row.leftContent}
                      type={row.type}
                      side="left"
                    />
                    <DiffCell
                      number={row.rightNumber}
                      content={row.rightContent}
                      type={row.type}
                      side="right"
                    />
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ))}
    </div>
  );
}
