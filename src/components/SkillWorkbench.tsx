import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Archive,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock,
  Code2,
  Eye,
  FileText,
  Folder,
  FolderOpen,
  GitBranch,
  GitCompare,
  History,
  Loader2,
  RotateCcw,
  Save,
  ShieldCheck,
} from "lucide-react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { cn } from "../utils";
import * as api from "../lib/tauri";
import type {
  ManagedSkill,
  SkillAuditEntry,
  SkillFileContent,
  SkillFileDiff,
  SkillFileNode,
  SkillQualityIssue,
  SkillEditSnapshot,
  GitBackupVersion,
} from "../lib/tauri";
import { SkillMarkdown } from "./SkillMarkdown";
import { SkillCodeEditor } from "./SkillCodeEditor";
import { DocumentDiffViewer } from "./DocumentDiffViewer";

type WorkbenchTab = "preview" | "edit" | "diff" | "quality" | "history";

interface Props {
  skill: ManagedSkill;
  onSaved?: (skill: ManagedSkill) => void;
}

function flattenFiles(nodes: SkillFileNode[]): SkillFileNode[] {
  const files: SkillFileNode[] = [];
  for (const node of nodes) {
    if (node.kind === "file") files.push(node);
    if (node.children) files.push(...flattenFiles(node.children));
  }
  return files;
}

function preferredFile(nodes: SkillFileNode[]) {
  const files = flattenFiles(nodes);
  const preferred = ["SKILL.md", "skill.md", "README.md", "readme.md"];
  return (
    files.find((file) => preferred.includes(file.name)) ??
    files.find((file) => /\.(md|markdown|txt|json|ya?ml)$/i.test(file.name)) ??
    files[0] ??
    null
  );
}

function isMarkdown(path: string) {
  return /\.(md|markdown)$/i.test(path);
}

function formatSize(size: number | null | undefined) {
  if (size == null) return "";
  if (size > 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size > 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${size} B`;
}

function formatTime(seconds: number) {
  return new Date(seconds > 9_999_999_999 ? seconds : seconds * 1000).toLocaleString();
}

function normalizeLineEndings(value: string) {
  return value.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

function detectPreferredEol(value: string) {
  return value.includes("\r\n") ? "\r\n" : "\n";
}

function serializeDraftForFile(file: SkillFileContent, draft: string) {
  const normalizedDraft = normalizeLineEndings(draft);
  const eol = detectPreferredEol(file.content);
  return eol === "\r\n" ? normalizedDraft.replace(/\n/g, "\r\n") : normalizedDraft;
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

export function SkillWorkbench({ skill, onSaved }: Props) {
  const [tree, setTree] = useState<SkillFileNode[]>([]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [file, setFile] = useState<SkillFileContent | null>(null);
  const [draft, setDraft] = useState("");
  const [diff, setDiff] = useState<SkillFileDiff | null>(null);
  const [issues, setIssues] = useState<SkillQualityIssue[]>([]);
  const [history, setHistory] = useState<SkillAuditEntry[]>([]);
  const [snapshots, setSnapshots] = useState<SkillEditSnapshot[]>([]);
  const [gitVersions, setGitVersions] = useState<GitBackupVersion[]>([]);
  const [tab, setTab] = useState<WorkbenchTab>("preview");
  const [loadingTree, setLoadingTree] = useState(true);
  const [loadingFile, setLoadingFile] = useState(false);
  const [saving, setSaving] = useState(false);
  const [restoringSnapshotId, setRestoringSnapshotId] = useState<string | null>(null);
  const [restoringGitTag, setRestoringGitTag] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const dirty = !!file && draft !== file.content;

  useEffect(() => {
    let stale = false;
    setLoadingTree(true);
    setError(null);
    Promise.all([
      api.getSkillFileTree(skill.id),
      api.checkSkillQuality(skill.id).catch(() => []),
      api.getSkillAuditHistory(skill.id).catch(() => []),
      api.listSkillEditSnapshots(skill.id).catch(() => []),
      api.gitBackupListVersions(12).catch(() => []),
    ])
      .then(([nextTree, nextIssues, nextHistory, nextSnapshots, nextGitVersions]) => {
        if (stale) return;
        setTree(nextTree);
        setIssues(nextIssues);
        setHistory(nextHistory);
        setSnapshots(nextSnapshots);
        setGitVersions(nextGitVersions);
        const first = preferredFile(nextTree);
        setExpanded(new Set(nextTree.filter((node) => node.kind === "directory").map((node) => node.relative_path)));
        setSelectedPath(first?.relative_path ?? null);
      })
      .catch((err) => {
        if (!stale) setError(err?.message ?? "Failed to load skill files");
      })
      .finally(() => {
        if (!stale) setLoadingTree(false);
      });
    return () => {
      stale = true;
    };
  }, [skill.id, skill.updated_at]);

  useEffect(() => {
    if (!selectedPath) {
      setFile(null);
      setDraft("");
      setDiff(null);
      return;
    }
    let stale = false;
    setLoadingFile(true);
    setError(null);
    api.readSkillFile(skill.id, selectedPath)
      .then((nextFile) => {
        if (stale) return;
        setFile(nextFile);
        setDraft(nextFile.content);
        setDiff(null);
        setTab((current) => (current === "diff" ? "preview" : current));
      })
      .catch((err) => {
        if (!stale) {
          setFile(null);
          setDraft("");
          setDiff(null);
          setError(err?.message ?? "Failed to read file");
        }
      })
      .finally(() => {
        if (!stale) setLoadingFile(false);
      });
    return () => {
      stale = true;
    };
  }, [selectedPath, skill.id]);

  const handleSelect = useCallback((node: SkillFileNode) => {
    if (dirty && !window.confirm("Discard unsaved changes and switch files?")) return;
    setSelectedPath(node.relative_path);
  }, [dirty]);

  const handleToggle = useCallback((path: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const previewSave = useCallback(async () => {
    if (!file) return;
    setSaving(true);
    try {
      const nextDiff = await api.previewSkillFileSave(skill.id, file.relative_path, serializeDraftForFile(file, draft), file.hash);
      setDiff(nextDiff);
      setTab("diff");
    } catch (err: unknown) {
      toast.error(err instanceof Error ? err.message : "Failed to build diff");
    } finally {
      setSaving(false);
    }
  }, [draft, file, skill.id]);

  const confirmSave = useCallback(async () => {
    if (!file) return;
    setSaving(true);
    try {
      const updated = await api.saveSkillFile(skill.id, file.relative_path, serializeDraftForFile(file, draft), file.hash);
      const [nextTree, nextFile, nextIssues, nextHistory, nextSnapshots, nextGitVersions] = await Promise.all([
        api.getSkillFileTree(skill.id),
        api.readSkillFile(skill.id, file.relative_path),
        api.checkSkillQuality(skill.id).catch(() => []),
        api.getSkillAuditHistory(skill.id).catch(() => []),
        api.listSkillEditSnapshots(skill.id).catch(() => []),
        api.gitBackupListVersions(12).catch(() => []),
      ]);
      setTree(nextTree);
      setFile(nextFile);
      setDraft(nextFile.content);
      setIssues(nextIssues);
      setHistory(nextHistory);
      setSnapshots(nextSnapshots);
      setGitVersions(nextGitVersions);
      setDiff(null);
      setTab("preview");
      onSaved?.(updated);
      toast.success("Skill file saved and synced to enabled targets");
    } catch (err: unknown) {
      toast.error(err instanceof Error ? err.message : "Failed to save file");
    } finally {
      setSaving(false);
    }
  }, [draft, file, onSaved, skill.id]);

  const restoreSnapshot = useCallback(async (snapshot: SkillEditSnapshot) => {
    if (!window.confirm(`Restore ${snapshot.relative_path} from this local edit snapshot?`)) return;
    setRestoringSnapshotId(snapshot.id);
    try {
      const updated = await api.restoreSkillEditSnapshot(skill.id, snapshot.id);
      const [nextTree, nextIssues, nextHistory, nextSnapshots] = await Promise.all([
        api.getSkillFileTree(skill.id),
        api.checkSkillQuality(skill.id).catch(() => []),
        api.getSkillAuditHistory(skill.id).catch(() => []),
        api.listSkillEditSnapshots(skill.id).catch(() => []),
      ]);
      setTree(nextTree);
      setIssues(nextIssues);
      setHistory(nextHistory);
      setSnapshots(nextSnapshots);
      setSelectedPath(snapshot.relative_path);
      const nextFile = await api.readSkillFile(skill.id, snapshot.relative_path);
      setFile(nextFile);
      setDraft(nextFile.content);
      setDiff(null);
      setTab("preview");
      onSaved?.(updated);
      toast.success("Snapshot restored and synced to enabled targets");
    } catch (err: unknown) {
      toast.error(err instanceof Error ? err.message : "Failed to restore snapshot");
    } finally {
      setRestoringSnapshotId(null);
    }
  }, [onSaved, skill.id]);

  const restoreGitVersion = useCallback(async (version: GitBackupVersion) => {
    if (!window.confirm("Restore the whole skill library to this Git snapshot and sync it to the configured remote?")) return;
    setRestoringGitTag(version.tag);
    try {
      await api.gitBackupRestoreVersion(version.tag);
      await api.gitBackupSync(`Restore skill library snapshot ${version.tag}`).catch(() => undefined);
      const [nextTree, nextIssues, nextHistory, nextSnapshots, nextGitVersions] = await Promise.all([
        api.getSkillFileTree(skill.id),
        api.checkSkillQuality(skill.id).catch(() => []),
        api.getSkillAuditHistory(skill.id).catch(() => []),
        api.listSkillEditSnapshots(skill.id).catch(() => []),
        api.gitBackupListVersions(12).catch(() => []),
      ]);
      setTree(nextTree);
      setIssues(nextIssues);
      setHistory(nextHistory);
      setSnapshots(nextSnapshots);
      setGitVersions(nextGitVersions);
      const nextSelected = selectedPath ?? preferredFile(nextTree)?.relative_path ?? null;
      setSelectedPath(nextSelected);
      if (nextSelected) {
        const nextFile = await api.readSkillFile(skill.id, nextSelected);
        setFile(nextFile);
        setDraft(nextFile.content);
      }
      setDiff(null);
      setTab("preview");
      toast.success("Git snapshot restored and pushed when a remote is configured");
    } catch (err: unknown) {
      toast.error(err instanceof Error ? err.message : "Failed to restore Git snapshot");
    } finally {
      setRestoringGitTag(null);
    }
  }, [selectedPath, skill.id]);

  const exportArchive = useCallback(async () => {
    setExporting(true);
    try {
      const path = await api.exportSkillArchive(skill.id);
      await revealItemInDir(path).catch(() => {});
      toast.success(`Exported to ${path}`);
    } catch (err: unknown) {
      toast.error(err instanceof Error ? err.message : "Failed to export skill");
    } finally {
      setExporting(false);
    }
  }, [skill.id]);

  const tabs = useMemo(() => [
    { id: "preview" as const, label: "Preview", icon: Eye },
    { id: "edit" as const, label: "Edit", icon: Code2 },
    { id: "diff" as const, label: "Diff", icon: GitCompare },
    { id: "quality" as const, label: "Quality", icon: ShieldCheck },
    { id: "history" as const, label: "History", icon: History },
  ], []);

  const handlePrimarySave = tab === "diff" && diff ? confirmSave : previewSave;

  return (
    <div className="grid min-h-[620px] grid-cols-[260px_minmax(0,1fr)] overflow-hidden rounded-xl border border-border-subtle bg-surface">
      <aside className="min-h-0 border-r border-border-subtle bg-bg-secondary">
        <div className="flex h-11 items-center justify-between border-b border-border-subtle px-3">
          <div className="flex min-w-0 items-center gap-2 text-[13px] font-semibold text-secondary">
            <Folder className="h-4 w-4 text-accent" />
            Files
          </div>
          <button
            type="button"
            onClick={exportArchive}
            disabled={exporting}
            className="rounded p-1.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
            title="Export skill archive"
          >
            {exporting ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Archive className="h-3.5 w-3.5" />}
          </button>
        </div>
        <div className="h-[calc(100%-2.75rem)] overflow-y-auto p-2">
          {loadingTree ? (
            <div className="flex justify-center py-8 text-muted">
              <Loader2 className="h-4 w-4 animate-spin" />
            </div>
          ) : tree.length > 0 ? (
            tree.map((node) => (
              <FileTreeNode
                key={node.relative_path}
                node={node}
                selectedPath={selectedPath}
                expanded={expanded}
                onToggle={handleToggle}
                onSelect={handleSelect}
              />
            ))
          ) : (
            <div className="px-3 py-8 text-center text-[13px] text-muted">No files found</div>
          )}
        </div>
      </aside>

      <section className="flex min-h-0 flex-col">
        <div className="flex min-h-11 flex-wrap items-center justify-between gap-2 border-b border-border-subtle px-3 py-2">
          <div className="flex min-w-0 items-center gap-2">
            {tabs.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setTab(item.id)}
                  className={cn(
                    "inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] font-medium transition-colors",
                    tab === item.id ? "bg-surface-active text-secondary" : "text-muted hover:bg-surface-hover hover:text-secondary"
                  )}
                >
                  <Icon className="h-3.5 w-3.5" />
                  {item.label}
                </button>
              );
            })}
          </div>
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate font-mono text-[12px] text-muted" title={file?.relative_path ?? selectedPath ?? ""}>
              {file?.relative_path ?? selectedPath ?? "No file selected"}
            </span>
            {dirty ? <span className="rounded bg-amber-500/10 px-1.5 py-0.5 text-[11px] text-amber-400">Unsaved</span> : null}
            <button
              type="button"
              onClick={handlePrimarySave}
              disabled={!dirty || saving || !file}
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-accent-border bg-accent-dark px-2.5 text-[12.5px] font-medium text-white transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            >
              {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
              Save
            </button>
          </div>
        </div>

        {error ? (
          <div className="m-4 rounded-lg border border-red-500/20 bg-red-500/10 px-4 py-3 text-[13px] text-red-300">
            {error}
          </div>
        ) : null}

        <div className="min-h-0 flex-1 overflow-auto bg-bg-secondary">
          {loadingFile ? (
            <div className="flex h-full items-center justify-center text-muted">
              <Loader2 className="h-5 w-5 animate-spin" />
            </div>
          ) : !file && tab !== "quality" && tab !== "history" ? (
            <div className="flex h-full items-center justify-center text-[13px] text-muted">Select a text file to preview.</div>
          ) : tab === "preview" && file ? (
            isMarkdown(file.relative_path) ? (
              <div className="p-6">
                <SkillMarkdown content={file.content} />
              </div>
            ) : (
              <SkillCodeEditor value={file.content} relativePath={file.relative_path} readOnly />
            )
          ) : tab === "edit" && file ? (
            <SkillCodeEditor value={draft} relativePath={file.relative_path} onChange={setDraft} />
          ) : tab === "diff" && file ? (
            <div className="space-y-4 p-4">
              <DocumentDiffViewer original={diff?.original_text ?? file.content} updated={diff?.updated_text ?? draft} />
              {dirty ? (
                <div className="flex justify-end">
                  <button
                    type="button"
                    onClick={confirmSave}
                    disabled={saving}
                    className="inline-flex h-9 items-center gap-2 rounded-lg border border-accent-border bg-accent-dark px-3 text-[13px] font-medium text-white hover:bg-accent disabled:opacity-50"
                  >
                    {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <CheckCircle2 className="h-3.5 w-3.5" />}
                    Confirm Save
                  </button>
                </div>
              ) : null}
            </div>
          ) : tab === "quality" ? (
            <div className="space-y-2 p-4">
              {issues.length === 0 ? (
                <div className="rounded-lg border border-emerald-500/20 bg-emerald-500/10 px-4 py-3 text-[13px] text-emerald-300">
                  No quality issues found.
                </div>
              ) : (
                issues.map((issue, index) => (
                  <button
                    key={`${issue.code}-${issue.relative_path}-${issue.line}-${index}`}
                    type="button"
                    onClick={() => issue.relative_path && setSelectedPath(issue.relative_path)}
                    className="flex w-full items-start gap-3 rounded-lg border border-border-subtle bg-surface px-3 py-2 text-left"
                  >
                    <IssueIcon severity={issue.severity} />
                    <span className="min-w-0 flex-1">
                      <span className="block text-[13px] font-medium text-secondary">{issue.message}</span>
                      <span className="mt-0.5 block font-mono text-[12px] text-muted">
                        {issue.relative_path ?? "skill"}{issue.line ? `:${issue.line}` : ""} | {issue.code}
                      </span>
                    </span>
                  </button>
                ))
              )}
            </div>
          ) : tab === "history" ? (
            <div className="space-y-4 p-4">
              <section className="space-y-2">
                <div className="flex items-center justify-between">
                  <h3 className="text-[13px] font-semibold text-secondary">Local Edit Snapshots</h3>
                  <span className="text-[12px] text-muted">{snapshots.length} available</span>
                </div>
                {snapshots.length === 0 ? (
                  <div className="rounded-lg border border-border-subtle bg-surface px-4 py-3 text-[13px] text-muted">
                    Saving an edited file will create a rollback snapshot here.
                  </div>
                ) : (
                  snapshots.map((snapshot) => (
                    <div key={snapshot.id} className="flex items-start gap-3 rounded-lg border border-border-subtle bg-surface px-3 py-2">
                      <RotateCcw className="mt-0.5 h-3.5 w-3.5 text-muted" />
                      <div className="min-w-0 flex-1">
                        <div className="truncate font-mono text-[12.5px] text-secondary" title={snapshot.relative_path}>
                          {snapshot.relative_path}
                        </div>
                        <div className="mt-0.5 text-[12px] text-muted">
                          {formatTime(snapshot.ts)} | {formatSize(snapshot.size)}
                        </div>
                      </div>
                      <button
                        type="button"
                        onClick={() => restoreSnapshot(snapshot)}
                        disabled={restoringSnapshotId === snapshot.id}
                        className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md border border-border-subtle bg-bg-secondary px-2.5 text-[12.5px] font-medium text-secondary hover:bg-surface-hover disabled:opacity-50"
                      >
                        {restoringSnapshotId === snapshot.id ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <RotateCcw className="h-3.5 w-3.5" />}
                        Restore
                      </button>
                    </div>
                  ))
                )}
              </section>

              <section className="space-y-2">
                <div className="flex items-center justify-between">
                  <h3 className="text-[13px] font-semibold text-secondary">Git Snapshot Rollback</h3>
                  <span className="text-[12px] text-muted">{gitVersions.length} versions</span>
                </div>
                {gitVersions.length === 0 ? (
                  <div className="rounded-lg border border-border-subtle bg-surface px-4 py-3 text-[13px] text-muted">
                    Git backup versions will appear here after backup snapshots are created.
                  </div>
                ) : (
                  gitVersions.map((version) => (
                    <div key={version.tag} className="flex items-start gap-3 rounded-lg border border-border-subtle bg-surface px-3 py-2">
                      <GitBranch className="mt-0.5 h-3.5 w-3.5 text-muted" />
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[13px] font-semibold text-secondary" title={version.message}>
                          {version.message || version.tag}
                        </div>
                        <div className="mt-0.5 truncate font-mono text-[12px] text-muted" title={version.tag}>
                          {version.tag} | {version.committed_at}
                        </div>
                      </div>
                      <button
                        type="button"
                        onClick={() => restoreGitVersion(version)}
                        disabled={restoringGitTag === version.tag}
                        className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md border border-border-subtle bg-bg-secondary px-2.5 text-[12.5px] font-medium text-secondary hover:bg-surface-hover disabled:opacity-50"
                      >
                        {restoringGitTag === version.tag ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <RotateCcw className="h-3.5 w-3.5" />}
                        Restore + Sync
                      </button>
                    </div>
                  ))
                )}
              </section>

              <section className="space-y-2">
                <h3 className="text-[13px] font-semibold text-secondary">Audit Log</h3>
                {history.length === 0 ? (
                  <div className="rounded-lg border border-border-subtle bg-surface px-4 py-3 text-[13px] text-muted">
                    No audit entries recorded for this skill yet.
                  </div>
                ) : (
                  history.map((entry) => (
                    <div key={entry.id} className="flex items-start gap-3 rounded-lg border border-border-subtle bg-surface px-3 py-2">
                      <Clock className="mt-0.5 h-3.5 w-3.5 text-muted" />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-[13px] font-semibold text-secondary">{entry.action}</span>
                          <span className={cn("rounded px-1.5 py-0.5 text-[11px]", entry.success ? "bg-emerald-500/10 text-emerald-400" : "bg-red-500/10 text-red-400")}>
                            {entry.success ? "success" : "failed"}
                          </span>
                          {entry.tool ? <span className="text-[12px] text-muted">{entry.tool}</span> : null}
                        </div>
                        <div className="mt-0.5 text-[12px] text-muted">{formatTime(entry.ts)}</div>
                        {entry.detail ? <div className="mt-1 font-mono text-[12px] text-tertiary">{entry.detail}</div> : null}
                      </div>
                    </div>
                  ))
                )}
              </section>
            </div>
          ) : null}
        </div>
      </section>
    </div>
  );
}
