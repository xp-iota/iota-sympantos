import { useEffect, useState } from "react";
import { getConfig, saveBackendModel } from "../api";
import type { BackendCheckResult, DesktopConfigSnapshot, DesktopModelConfig } from "../types";

type Props = {
  config?: DesktopConfigSnapshot | null;
  backendChecks?: Record<string, BackendCheckResult>;
  onConfigUpdate?: (config: DesktopConfigSnapshot) => void;
};

export function ConfigPanel({ config: externalConfig, backendChecks = {}, onConfigUpdate }: Props) {
  const [config, setConfig] = useState<DesktopConfigSnapshot | null>(null);
  const [edits, setEdits] = useState<Record<string, Partial<DesktopModelConfig>>>({});

  useEffect(() => {
    if (externalConfig) {
      setConfig(externalConfig);
      return;
    }
    getConfig()
      .then((snapshot) => {
        setConfig(snapshot);
        onConfigUpdate?.(snapshot);
      })
      .catch((err) => console.error(err));
  }, [externalConfig, onConfigUpdate]);

  if (!config) {
    return <div className="p-4 text-sm text-gray-500">Loading config...</div>;
  }

  return (
    <div className="h-full overflow-y-auto bg-app-canvas">
      <div className="mx-auto max-w-3xl px-6 py-8">
        <h2 className="text-[15px] font-extrabold uppercase tracking-wider text-slate-300">Configuration</h2>
        <p className="mt-1.5 text-xs text-slate-500 font-mono" title={config.config_path}>
          Path: {config.config_path}
        </p>
        <div className="mt-6 space-y-4">
          {Object.values(config.backends).map((backend) => {
            const draft = edits[backend.backend] ?? {};
            const model = backend.model;
            const check = backendChecks[backend.backend];
            const updateDraft = (patch: Partial<DesktopModelConfig>) =>
              setEdits({ ...edits, [backend.backend]: { ...draft, ...patch } });

            return (
              <div key={backend.backend} className="rounded-xl border border-slate-850 bg-slate-950/20 p-5 shadow-sm">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-sm font-bold uppercase tracking-wider text-slate-200">{backend.backend}</div>
                    <div className="text-xs text-slate-500 font-mono mt-0.5">
                      {backend.model?.name ?? "No model"} · API key{" "}
                      {backend.model?.api_key_configured ? "configured" : "missing"}
                    </div>
                  </div>
                  <span className={`px-2.5 py-0.5 rounded-md text-[10px] font-bold border ${
                    backend.enabled
                      ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/20"
                      : "bg-slate-950/40 text-slate-500 border-slate-850"
                  }`}>
                    {backend.enabled ? "ENABLED" : "DISABLED"}
                  </span>
                </div>
                <div
                  className={`mt-4 rounded-lg border px-4 py-2.5 text-xs font-mono leading-relaxed ${
                    check?.ok
                      ? "border-emerald-500/20 bg-emerald-500/5 text-emerald-400"
                      : check
                        ? "border-amber-500/20 bg-amber-500/5 text-amber-400"
                        : "border-slate-850 bg-slate-955/20 text-slate-500"
                  }`}
                >
                  {check ? check.details : "Checking backend readiness..."}
                </div>
                <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2">
                  <input
                    value={draft.provider ?? model?.provider ?? ""}
                    onChange={(event) => updateDraft({ provider: event.target.value })}
                    placeholder="Provider"
                    className="rounded-lg border border-slate-800 bg-slate-955/40 px-3.5 py-2.5 text-xs text-slate-200 outline-none focus:border-primary/60 transition-all font-mono"
                  />
                  <input
                    value={draft.name ?? model?.name ?? ""}
                    onChange={(event) => updateDraft({ name: event.target.value })}
                    placeholder="Model name"
                    className="rounded-lg border border-slate-800 bg-slate-955/40 px-3.5 py-2.5 text-xs text-slate-200 outline-none focus:border-primary/60 transition-all font-mono"
                  />
                  <input
                    value={draft.base_url ?? model?.base_url ?? ""}
                    onChange={(event) => updateDraft({ base_url: event.target.value })}
                    placeholder="Base URL"
                    className="rounded-lg border border-slate-800 bg-slate-955/40 px-3.5 py-2.5 text-xs text-slate-200 outline-none focus:border-primary/60 transition-all font-mono md:col-span-2"
                  />
                  <input
                    type="password"
                    value={draft.api_key_update ?? ""}
                    onChange={(event) => updateDraft({ api_key_update: event.target.value })}
                    placeholder="Update API key"
                    className="rounded-lg border border-slate-800 bg-slate-955/40 px-3.5 py-2.5 text-xs text-slate-200 outline-none focus:border-primary/60 transition-all font-mono"
                  />
                  <button
                    className="rounded-lg bg-primary hover:bg-primary-hover px-4 py-2.5 text-xs text-white font-semibold shadow-sm shadow-primary/20 transition-all cursor-pointer"
                    onClick={async () => {
                      const apiKey = draft.api_key_update?.trim();
                      const updated = await saveBackendModel(backend.backend, {
                        api_key_configured: Boolean(backend.model?.api_key_configured),
                        ...(draft.provider !== undefined ? { provider: draft.provider } : {}),
                        ...(draft.name !== undefined ? { name: draft.name } : {}),
                        ...(draft.base_url !== undefined ? { base_url: draft.base_url } : {}),
                        ...(apiKey ? { api_key_update: apiKey } : {}),
                      });
                      setConfig(updated);
                      onConfigUpdate?.(updated);
                      setEdits({ ...edits, [backend.backend]: {} });
                    }}
                  >
                    Save Changes
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
