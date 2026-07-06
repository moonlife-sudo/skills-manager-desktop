import { useEffect, useMemo, useRef } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";
import { json } from "@codemirror/lang-json";
import { yaml } from "@codemirror/lang-yaml";

interface Props {
  value: string;
  relativePath?: string;
  readOnly?: boolean;
  onChange?: (value: string) => void;
}

function languageForPath(path?: string): Extension[] {
  const lower = (path ?? "").toLowerCase();
  if (lower.endsWith(".md") || lower.endsWith(".markdown")) return [markdown()];
  if (lower.endsWith(".json") || lower.endsWith(".jsonc")) return [json()];
  if (lower.endsWith(".yaml") || lower.endsWith(".yml")) return [yaml()];
  return [];
}

export function SkillCodeEditor({ value, relativePath, readOnly, onChange }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);

  const extensions = useMemo<Extension[]>(() => [
    lineNumbers(),
    EditorView.lineWrapping,
    EditorView.editable.of(!readOnly),
    EditorState.readOnly.of(!!readOnly),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        onChange?.(update.state.doc.toString());
      }
    }),
    EditorView.theme({
      "&": {
        height: "100%",
        backgroundColor: "var(--color-bg-secondary)",
        color: "var(--color-text-secondary)",
        fontSize: "13px",
      },
      ".cm-scroller": {
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
        lineHeight: "1.6",
      },
      ".cm-gutters": {
        backgroundColor: "var(--color-surface)",
        color: "var(--color-text-faint)",
        borderRight: "1px solid var(--color-border-subtle)",
      },
      ".cm-activeLine, .cm-activeLineGutter": {
        backgroundColor: "var(--color-surface-hover)",
      },
      ".cm-content": {
        padding: "12px 0",
      },
      ".cm-line": {
        padding: "0 14px",
      },
      "&.cm-focused": {
        outline: "none",
      },
    }),
    ...languageForPath(relativePath),
  ], [onChange, readOnly, relativePath]);

  useEffect(() => {
    if (!hostRef.current) return;
    const view = new EditorView({
      parent: hostRef.current,
      state: EditorState.create({ doc: value, extensions }),
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // CodeMirror owns live document edits; prop updates are applied by the sync effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [extensions]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (current === value) return;
    view.dispatch({
      changes: { from: 0, to: current.length, insert: value },
    });
  }, [value]);

  return <div ref={hostRef} className="h-full min-h-0 overflow-hidden" />;
}
