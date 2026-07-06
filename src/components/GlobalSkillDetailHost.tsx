import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { SkillToolToggle } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import { SkillDetailPanel } from "./SkillDetailPanel";

export function GlobalSkillDetailHost() {
  const { t } = useTranslation();
  const {
    viewedPreset,
    tools,
    managedSkills,
    detailSkillId,
    closeSkillDetail,
    refreshManagedSkills,
    projects,
    refreshProjects,
  } = useApp();
  const [toolToggles, setToolToggles] = useState<SkillToolToggle[] | null>(null);
  const [togglingToolKey, setTogglingToolKey] = useState<string | null>(null);

  const selectedSkill = useMemo(
    () => managedSkills.find((skill) => skill.id === detailSkillId) || null,
    [detailSkillId, managedSkills]
  );

  useEffect(() => {
    let cancelled = false;
    const loadToggles = async () => {
      if (!selectedSkill || !viewedPreset || !selectedSkill.preset_ids.includes(viewedPreset.id)) {
        setToolToggles(null);
        return;
      }
      try {
        const toggles = await api.getSkillToolToggles(selectedSkill.id, viewedPreset.id);
        if (!cancelled) setToolToggles(toggles);
      } catch {
        if (!cancelled) setToolToggles(null);
      }
    };
    void loadToggles();
    return () => {
      cancelled = true;
    };
  }, [selectedSkill, viewedPreset]);

  const handleToggleSkillTool = async (toolKey: string, enabled: boolean) => {
    if (!selectedSkill || !viewedPreset) return;
    setTogglingToolKey(toolKey);
    try {
      await api.setSkillToolToggle(selectedSkill.id, viewedPreset.id, toolKey, enabled);
      const displayName = tools.find((tool) => tool.key === toolKey)?.display_name ?? toolKey;
      toast.success(
        enabled
          ? t("mySkills.agentToggleEnabled", { agent: displayName })
          : t("mySkills.agentToggleDisabled", { agent: displayName })
      );
      const [, toggles] = await Promise.all([
        refreshManagedSkills(),
        api.getSkillToolToggles(selectedSkill.id, viewedPreset.id),
      ]);
      setToolToggles(toggles);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
      await refreshManagedSkills();
    } finally {
      setTogglingToolKey(null);
    }
  };

  return (
    <SkillDetailPanel
      key={selectedSkill?.id ?? "global-skill-detail-empty"}
      skill={selectedSkill}
      onClose={closeSkillDetail}
      tools={tools}
      toolToggles={toolToggles}
      togglingTool={togglingToolKey}
      onToggleTool={handleToggleSkillTool}
      projects={projects}
      onProjectsChanged={refreshProjects}
    />
  );
}
