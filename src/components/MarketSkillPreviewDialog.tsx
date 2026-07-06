import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  DownloadCloud,
  FileText,
  Folder,
  FolderOpen,
  Loader2,
  ShieldCheck,
  Tag,
} from "lucide-react";
import { toast } from "sonner";
import { cn } from "../utils";
import * as api from "../lib/tauri";
import type {
  MarketSkillPreview,
  Preset,
  SkillFileNode,
  SkillQualityIssue,
  SkillsShSkill,
} from "../lib/tauri";
import { DetailSheet } from "./DetailSheet";
import { SkillMarkdown } from "./SkillMarkdown";
import { SkillCodeEditor } from "./SkillCodeEditor";

const previewCache = new Map<string, MarketSkillPreview>();
const MAX_PREVIEW_CACHE_ITEMS = 24;

interface MarketInstallOptions {
  name: string;
  tags: string[];
  presetId: string | null;
}

interface Props {
  skill: SkillsShSkill | null;
  installed: boolean;
  installing: boolean;
  presets: Preset[];
  defaultPresetId: string | null;
  onClose: () => void;
  onInstall: (skill: SkillsShSkill, options: MarketInstallOptions) => Promise<void>;
}

function flattenFiles(nodes: SkillFileNode[]): SkillFileNode[] {
  const files: SkillFileNode[] = [];
  for (const node of nodes) {
    if (node.kind === "file") files.push(node);
    if (node.children) files.push(...flattenFiles(node.children));
  }
  return files;
}

function formatSize(size: number | null | undefined) {
  if (size == null) return "";
  if (size > 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size > 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${size} B`;
}

function IssueIcon({ severity }: { severity: SkillQualityIssue["severity"] }) {
  if (severity === "error") return <AlertTriangle className="h-3.5 w-3.5 text-red-400" />;
  if (severity === "warning") return <AlertTriangle className="h-3.5 w-3.5 text-amber-400" />;
  return <ShieldCheck className="h-3.5 w-3.5 text-sky-400" />;
}

function FileTreeNode({
  node,
  selectedPath,
  expanded,
  onToggle,
  onSelect,
}: {
  node: SkillFileNode;
  selectedPath: string | null;
  expanded: Set<string>;
  onToggle: (path: string) => void;
  onSelect: (node: SkillFileNode) => void;
}) {
  const isDir = node.kind === "directory";
  const isOpen = expanded.has(node.relative_path);
  const Icon = isDir ? (isOpen ? FolderOpen : Folder) : FileText;

  return (
    <div>
      <button
        type="button"
        onClick={() => (isDir ? onToggle(node.relative_path) : onSelect(node))}
        className={cn(
          "flex h-7 w-full min-w-0 items-center gap-1.5 rounded px-2 text-left text-[12.5px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary",
          selectedPath === node.relative_path && "bg-surface-active text-secondary"
        )}
        title={node.relative_path}
      >
        {isDir ? (
          isOpen ? <ChevronDown className="h-3 w-3 shrink-0" /> : <ChevronRight className="h-3 w-3 shrink-0" />
        ) : (
          <span className="w-3 shrink-0" />
        )}
        <Icon className="h-3.5 w-3.5 shrink-0" />
        <span className="truncate">{node.name}</span>
        {!isDir ? <span className="ml-auto shrink-0 text-[11px] text-faint">{formatSize(node.size)}</span> : null}
      </button>
      {isDir && isOpen && node.children ? (
        <div className="ml-3 border-l border-border-subtle pl-1">
          {node.children.map((child) => (
            <FileTreeNode
              key={child.relative_path}
              node={child}
              selectedPath={selectedPath}
              expanded={expanded}
              onToggle={onToggle}
              onSelect={onSelect}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function splitTags(value: string) {
  return value
    .split(/[,\n]/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function cachePreview(key: string, preview: MarketSkillPreview) {
  if (previewCache.has(key)) previewCache.delete(key);
  previewCache.set(key, preview);
  while (previewCache.size > MAX_PREVIEW_CACHE_ITEMS) {
    const first = previewCache.keys().next().value;
    if (!first) break;
    previewCache.delete(first);
  }
}

export function MarketSkillPreviewDialog({
  skill,
  installed,
  installing,
  presets,
  defaultPresetId,
  onClose,
  onInstall,
}: Props) {
  const [preview, setPreview] = useState<MarketSkillPreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [installName, setInstallName] = useState("");
  const [tagsInput, setTagsInput] = useState("market, skills.sh");
  const [addToPreset, setAddToPreset] = useState(Boolean(defaultPresetId));
  const [presetId, setPresetId] = useState(defaultPresetId ?? "");

  useEffect(() => {
    let stale = false;
    void Promise.resolve().then(async () => {
      if (stale) return;
      if (!skill) {
        setPreview(null);
        return;
      }

      setLoading(true);
      setError(null);
      setPreview(null);
      setInstallName(skill.name || skill.skill_id);
      setSelectedPath(null);
      setExpanded(new Set());
      setAddToPreset(Boolean(defaultPresetId));
      setPresetId(defaultPresetId ?? "");

      try {
        const cacheKey = `${skill.source}/${skill.skill_id}`;
        const cached = previewCache.get(cacheKey);
        if (cached) {
          setPreview(cached);
          setInstallName(cached.name || skill.name || skill.skill_id);
          setSelectedPath(cached.document?.relative_path ?? cached.file_previews[0]?.relative_path ?? null);
          setExpanded(new Set(cached.files.filter((node) => node.kind === "directory").map((node) => node.relative_path)));
          setLoading(false);
          return;
        }
        const nextPreview = await api.previewSkillsshSkill(skill.source, skill.skill_id);
        if (stale) return;
        cachePreview(cacheKey, nextPreview);
        setPreview(nextPreview);
        setInstallName(nextPreview.name || skill.name || skill.skill_id);
        setSelectedPath(nextPreview.document?.relative_path ?? nextPreview.file_previews[0]?.relative_path ?? null);
        setExpanded(new Set(nextPreview.files.filter((node) => node.kind === "directory").map((node) => node.relative_path)));
      } catch (err: unknown) {
        if (!stale) setError(err instanceof Error ? err.message : "Failed to load market preview");
      } finally {
        if (!stale) setLoading(false);
      }
    });

    return () => {
      stale = true;
    };
  }, [defaultPresetId, skill]);

  const selectedNode = useMemo(() => {
    if (!preview || !selectedPath) return null;
    return flattenFiles(preview.files).find((node) => node.relative_path === selectedPath) ?? null;
  }, [preview, selectedPath]);

  const selectedPreview = useMemo(() => {
    if (!preview || !selectedPath) return null;
    return preview.file_previews.find((item) => item.relative_path === selectedPath) ?? null;
  }, [preview, selectedPath]);

  const riskCounts = useMemo(() => {
    const issues = preview?.risk_issues ?? [];
    return {
      error: issues.filter((issue) => issue.severity === "error").length,
      warning: issues.filter((issue) => issue.severity === "warning").length,
    };
  }, [preview]);

  if (!skill) return null;

  const displayName = preview?.name || skill.name || skill.skill_id;
  const sourceRef = `${skill.source}/${skill.skill_id}`;
  const doc = preview?.document;
  const selectedContent = selectedPreview
    ? { content: selectedPreview.content, relative_path: selectedPreview.relative_path }
    : doc && selectedPath === doc.relative_path
      ? { content: doc.content, relative_path: doc.relative_path }
      : null;

  const handleInstall = async () => {
    if (!installName.trim()) {
      toast.error("Install name is required");
      return;
    }
    await onInstall(skill, {
      name: installName.trim(),
      tags: splitTags(tagsInput),
      presetId: addToPreset && presetId ? presetId : null,
    });
  };

  const meta = (
    <div className="flex flex-wrap items-center gap-2 text-[12.5px] text-muted">
      <span className="rounded-full border border-border-subtle bg-surface px-2 py-1 font-mono">
        {sourceRef}
      </span>
      {skill.installs > 0 ? (
        <span className="inline-flex items-center gap-1 rounded-full border border-border-subtle bg-surface px-2 py-1">
          <DownloadCloud className="h-3 w-3" />
          {skill.installs.toLocaleString()} installs
        </span>
      ) : null}
      {riskCounts.error > 0 || riskCounts.warning > 0 ? (
        <span className="inline-flex items-center gap-1 rounded-full border border-amber-500/20 bg-amber-500/10 px-2 py-1 text-amber-300">
          <AlertTriangle className="h-3 w-3" />
          {riskCounts.error} errors / {riskCounts.warning} warnings
        </span>
      ) : preview ? (
        <span className="inline-flex items-center gap-1 rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-1 text-emerald-300">
          <ShieldCheck className="h-3 w-3" />
          No obvious risks
        </span>
      ) : null}
    </div>
  );

  return (
    <DetailSheet
      open={true}
      title={displayName}
      description={preview?.description ? <p className="line-clamp-3">{preview.description}</p> : undefined}
      meta={meta}
      onClose={onClose}
    >
      {loading ? (
        <div className="flex items-center justify-center py-20 text-muted">
          <Loader2 className="h-5 w-5 animate-spin" />
        </div>
      ) : error ? (
        <div className="rounded-lg border border-red-500/20 bg-red-500/10 px-4 py-3 text-[13px] text-red-300">
          {error}
        </div>
      ) : preview ? (
        <div className="grid min-h-[600px] grid-cols-[260px_minmax(0,1fr)] overflow-hidden rounded-xl border border-border-subtle bg-surface">
          <aside className="min-h-0 border-r border-border-subtle bg-bg-secondary">
            <div className="flex h-11 items-center gap-2 border-b border-border-subtle px-3 text-[13px] font-semibold text-secondary">
              <Folder className="h-4 w-4 text-accent" />
              Files
            </div>
            <div className="h-[calc(100%-2.75rem)] overflow-y-auto p-2">
              {preview.files.map((node) => (
                <FileTreeNode
                  key={node.relative_path}
                  node={node}
                  selectedPath={selectedPath}
                  expanded={expanded}
                  onToggle={(path) => {
                    setExpanded((current) => {
                      const next = new Set(current);
                      if (next.has(path)) next.delete(path);
                      else next.add(path);
                      return next;
                    });
                  }}
                  onSelect={(node) => setSelectedPath(node.relative_path)}
                />
              ))}
            </div>
          </aside>

          <section className="flex min-h-0 flex-col">
            <div className="flex flex-wrap items-end gap-3 border-b border-border-subtle bg-bg-secondary px-3 py-3">
              <label className="min-w-[220px] flex-1">
                <span className="mb-1 block text-[12px] font-medium text-muted">Install name</span>
                <input
                  value={installName}
                  onChange={(event) => setInstallName(event.target.value)}
                  className="app-input h-9 w-full bg-surface"
                />
              </label>
              <label className="min-w-[220px] flex-1">
                <span className="mb-1 block text-[12px] font-medium text-muted">Tags</span>
                <div className="relative">
                  <Tag className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
                  <input
                    value={tagsInput}
                    onChange={(event) => setTagsInput(event.target.value)}
                    placeholder="comma separated"
                    className="app-input h-9 w-full bg-surface pl-9"
                  />
                </div>
              </label>
              <label className="flex min-w-[220px] flex-1 items-center gap-2 rounded-lg border border-border-subtle bg-surface px-3 py-2">
                <input
                  type="checkbox"
                  checked={addToPreset}
                  onChange={(event) => setAddToPreset(event.target.checked)}
                  className="h-4 w-4 accent-[var(--color-accent)]"
                />
                <span className="text-[13px] text-secondary">Add to preset</span>
                <select
                  value={presetId}
                  disabled={!addToPreset || presets.length === 0}
                  onChange={(event) => setPresetId(event.target.value)}
                  className="ml-auto h-8 min-w-0 rounded border border-border-subtle bg-bg-secondary px-2 text-[12.5px] text-secondary"
                >
                  {presets.length === 0 ? (
                    <option value="">No preset</option>
                  ) : (
                    presets.map((preset) => (
                      <option key={preset.id} value={preset.id}>
                        {preset.name}
                      </option>
                    ))
                  )}
                </select>
              </label>
              <button
                type="button"
                onClick={handleInstall}
                disabled={installed || installing}
                className="app-button-primary h-9 shrink-0"
              >
                {installed ? (
                  <>
                    <CheckCircle2 className="h-4 w-4" />
                    Installed
                  </>
                ) : installing ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin" />
                    Installing
                  </>
                ) : (
                  <>
                    <DownloadCloud className="h-4 w-4" />
                    Install after review
                  </>
                )}
              </button>
            </div>

            <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_280px] overflow-hidden">
              <div className="min-h-0 overflow-auto bg-bg-secondary">
                {selectedContent ? (
                  selectedContent.relative_path.toLowerCase().endsWith(".md") || selectedContent.relative_path.toLowerCase().endsWith(".markdown") ? (
                    <div className="p-6">
                      <SkillMarkdown content={selectedContent.content} />
                    </div>
                  ) : (
                    <SkillCodeEditor value={selectedContent.content} relativePath={selectedContent.relative_path} readOnly />
                  )
                ) : selectedNode ? (
                  <div className="flex h-full items-center justify-center px-8 text-center text-[13px] text-muted">
                    This file is binary, too large, or outside the preview size budget. Install review still checks every listed file.
                  </div>
                ) : (
                  <div className="flex h-full items-center justify-center text-[13px] text-muted">
                    Select a file to preview.
                  </div>
                )}
              </div>

              <aside className="min-h-0 overflow-y-auto border-l border-border-subtle bg-surface p-3">
                <h3 className="mb-2 text-[13px] font-semibold text-secondary">Install Review</h3>
                <div className="space-y-2">
                  {preview.risk_issues.length === 0 ? (
                    <div className="rounded-lg border border-emerald-500/20 bg-emerald-500/10 px-3 py-2 text-[12.5px] text-emerald-300">
                      No obvious quality or safety issues were found.
                    </div>
                  ) : (
                    preview.risk_issues.map((issue, index) => (
                      <button
                        key={`${issue.code}-${issue.relative_path}-${issue.line}-${index}`}
                        type="button"
                        onClick={() => issue.relative_path && setSelectedPath(issue.relative_path)}
                        className="flex w-full items-start gap-2 rounded-lg border border-border-subtle bg-bg-secondary px-2.5 py-2 text-left"
                      >
                        <IssueIcon severity={issue.severity} />
                        <span className="min-w-0">
                          <span className="block text-[12.5px] font-medium text-secondary">{issue.message}</span>
                          <span className="mt-0.5 block font-mono text-[11.5px] text-muted">
                            {issue.relative_path ?? "skill"}{issue.line ? `:${issue.line}` : ""} / {issue.code}
                          </span>
                        </span>
                      </button>
                    ))
                  )}
                </div>
              </aside>
            </div>
          </section>
        </div>
      ) : null}
    </DetailSheet>
  );
}
