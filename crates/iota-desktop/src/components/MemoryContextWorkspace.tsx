import { useEffect, useState } from "react";
import { getMemoryContextSnapshot } from "../api";
import type { DesktopMemoryContextSnapshot, DesktopMemoryRecord } from "../types";
import {
  Search,
  Database,
  Cpu,
  Layers,
  ChevronRight,
  Copy,
  Check,
  AlertCircle,
  RefreshCw,
  Clock,
  Terminal,
  Sliders
} from "lucide-react";

export type MemoryContextMode = "memory" | "context";

type MemoryContextWorkspaceProps = {
  mode?: MemoryContextMode;
};

export function MemoryContextWorkspace({ mode = "memory" }: MemoryContextWorkspaceProps) {
  const [scopeMode, setScopeMode] = useState<"workspace" | "all">("workspace");
  const [snapshot, setSnapshot] = useState<DesktopMemoryContextSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedBucket, setSelectedBucket] = useState<string>("identity");
  const [selectedRecord, setSelectedRecord] = useState<DesktopMemoryRecord | null>(null);
  const [copiedRecordId, setCopiedRecordId] = useState<string | null>(null);
  const [copiedCapsule, setCopiedCapsule] = useState(false);
  const [fullCapsuleExpanded, setFullCapsuleExpanded] = useState(false);

  const fetchSnapshot = async () => {
    setLoading(true);
    setError(null);
    try {
      const snap = await getMemoryContextSnapshot(scopeMode);
      setSnapshot(snap);
      
      // Keep selected bucket, or fallback to identity if not exist
      const records = snap.memory[selectedBucket as keyof typeof snap.memory] || [];
      if (records.length > 0) {
        setSelectedRecord(records[0]);
      } else {
        setSelectedRecord(null);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchSnapshot();
  }, [scopeMode]);

  // Auto-select first record when switching buckets or filtering
  useEffect(() => {
    if (snapshot) {
      const records = snapshot.memory[selectedBucket as keyof typeof snapshot.memory] || [];
      const filtered = records.filter(r =>
        r.content.toLowerCase().includes(searchQuery.toLowerCase())
      );
      if (filtered.length > 0) {
        setSelectedRecord(filtered[0]);
      } else {
        setSelectedRecord(null);
      }
    }
  }, [selectedBucket, searchQuery, snapshot]);

  const handleCopyRecord = (record: DesktopMemoryRecord) => {
    navigator.clipboard.writeText(record.content);
    setCopiedRecordId(record.id);
    setTimeout(() => setCopiedRecordId(null), 2000);
  };

  const handleCopyCapsule = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedCapsule(true);
    setTimeout(() => setCopiedCapsule(false), 2000);
  };

  const formatDate = (ts: number) => {
    if (ts <= 0) return "Never";
    return new Date(ts * 1000).toLocaleString();
  };

  const bucketsList = [
    { id: "identity", label: "Identity", count: snapshot?.memory_summary.identity || 0, color: "text-sky-400 bg-sky-500/10 border-sky-500/20" },
    { id: "preference", label: "Preference", count: snapshot?.memory_summary.preference || 0, color: "text-amber-400 bg-amber-500/10 border-amber-500/20" },
    { id: "strategic", label: "Strategic", count: snapshot?.memory_summary.strategic || 0, color: "text-cyan-400 bg-cyan-500/10 border-cyan-500/20" },
    { id: "domain", label: "Domain", count: snapshot?.memory_summary.domain || 0, color: "text-emerald-400 bg-emerald-500/10 border-emerald-500/20" },
    { id: "procedural", label: "Procedural", count: snapshot?.memory_summary.procedural || 0, color: "text-blue-400 bg-blue-500/10 border-blue-500/20" },
    { id: "episodic", label: "Episodic", count: snapshot?.memory_summary.episodic || 0, color: "text-indigo-400 bg-indigo-500/10 border-indigo-500/20" },
  ];

  const currentRecords = snapshot ? (snapshot.memory[selectedBucket as keyof typeof snapshot.memory] || []) : [];
  const filteredRecords = currentRecords.filter(r =>
    r.content.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const isMemoryMode = mode === "memory";

  return (
    <div className="flex flex-col h-full bg-app-canvas text-slate-200 overflow-hidden font-sans">
      {/* Top Header */}
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-slate-850 bg-app-surface/80 px-4 py-3 backdrop-blur-md shrink-0">
        <div className="flex items-center gap-3">
          <div className="rounded-lg bg-primary/10 p-2 text-primary border border-primary/25">
            {isMemoryMode ? <Database className="h-4.5 w-4.5" /> : <Cpu className="h-4.5 w-4.5" />}
          </div>
          <div>
            <h1 className="text-[13px] font-bold tracking-wider text-slate-250 uppercase flex items-center gap-1.5">
              {isMemoryMode ? "Persistent Memory" : "Runtime Context"}
              <span className="text-[9px] uppercase font-bold tracking-widest bg-slate-900/60 text-slate-400 px-2 py-0.5 rounded border border-slate-800/80">
                Read-Only
              </span>
            </h1>
            <p className="text-[11px] text-slate-500 leading-normal mt-0.5">
              {isMemoryMode
                ? "Inspect persistent identity, preference, strategic, domain, procedural, and episodic memories."
                : "Inspect the captured runtime context capsule and Context Fabric configuration."}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/* Scope Selection */}
          <div className="flex rounded-lg bg-slate-950/40 p-0.5 border border-slate-850">
            <button
              onClick={() => setScopeMode("workspace")}
              className={`rounded-md px-3 py-1 text-xs font-semibold transition-all cursor-pointer ${
                scopeMode === "workspace"
                  ? "bg-primary text-white shadow-sm shadow-primary/20 border border-primary/10"
                  : "text-slate-400 hover:text-slate-200"
              }`}
            >
              Workspace Scope
            </button>
            <button
              onClick={() => setScopeMode("all")}
              className={`rounded-md px-3 py-1 text-xs font-semibold transition-all cursor-pointer ${
                scopeMode === "all"
                  ? "bg-primary text-white shadow-sm shadow-primary/20 border border-primary/10"
                  : "text-slate-400 hover:text-slate-200"
              }`}
            >
              All Scope
            </button>
          </div>

          {/* Refresh Button */}
          <button
            onClick={fetchSnapshot}
            disabled={loading}
            className="flex items-center justify-center rounded-lg border border-slate-800 bg-slate-955/25 p-2 text-slate-400 hover:bg-slate-955/50 hover:text-slate-200 transition-all disabled:opacity-50 cursor-pointer"
            title="Refresh Snapshot"
          >
            <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin text-primary" : ""}`} />
          </button>
        </div>
      </header>

      {/* Errors Notification */}
      {snapshot?.errors && snapshot.errors.length > 0 && (
        <div className="bg-red-500/10 border-b border-red-500/20 px-6 py-2.5 flex items-center gap-3 text-xs text-red-300 shrink-0">
          <AlertCircle className="h-4 w-4 shrink-0 text-red-400" />
          <div className="flex-1 truncate">
            {snapshot.errors.map((err, idx) => (
              <span key={idx} className="mr-4">
                <strong>[{err.area}]</strong> {err.message}
              </span>
            ))}
          </div>
        </div>
      )}

      {error && (
        <div className="bg-red-500/10 border-b border-red-500/20 px-6 py-4 flex items-center gap-3 text-sm text-red-350 shrink-0">
          <AlertCircle className="h-5 w-5 shrink-0 text-red-400" />
          <div>
            <h4 className="font-semibold text-red-300">Error loading snapshot</h4>
            <p className="text-xs text-red-450 mt-0.5">{error}</p>
          </div>
        </div>
      )}

      <div className="flex-1 overflow-hidden">
        
        {isMemoryMode && (
        <section className="flex flex-col h-full overflow-hidden bg-app-canvas">
          <div className="flex items-center justify-between border-b border-slate-850 bg-slate-955/10 px-4 py-2.5 shrink-0">
            <span className="text-[11px] font-bold uppercase tracking-wider text-slate-400 flex items-center gap-2">
              <Database className="h-4 w-4 text-primary/80" /> Persistent Memory
            </span>
            <span className="text-[10px] text-slate-500 font-mono">
              CWD: {snapshot?.cwd || "Unknown"}
            </span>
          </div>

          <div className="flex flex-1 overflow-hidden">
            {/* Buckets Sidebar */}
            <div className="w-[140px] border-r border-slate-850 bg-slate-955/10 flex flex-col gap-1 p-2 overflow-y-auto shrink-0">
              {bucketsList.map((bucket) => {
                const isSelected = selectedBucket === bucket.id;
                return (
                  <button
                    key={bucket.id}
                    onClick={() => {
                      setSelectedBucket(bucket.id);
                      setSearchQuery("");
                    }}
                    className={`group w-full flex items-center justify-between rounded-md px-3 py-2.5 text-left text-xs font-semibold border border-transparent transition-all cursor-pointer ${
                      isSelected
                        ? "bg-primary/10 text-primary-hover border-primary/25 shadow-sm"
                        : "text-slate-400 hover:bg-slate-955/20 hover:text-slate-200"
                    }`}
                  >
                    <span className="truncate">{bucket.label}</span>
                    <span className={`rounded-full px-2 py-0.5 text-[10px] font-bold font-mono ${
                      isSelected ? "bg-primary/20 text-primary-hover" : "bg-slate-950/40 text-slate-500 group-hover:text-slate-300"
                    }`}>
                      {bucket.count}
                    </span>
                  </button>
                );
              })}
            </div>

            {/* Records Column */}
            <div className="flex-1 flex flex-col h-full overflow-hidden bg-app-canvas/25">
              {/* Search Bar */}
              <div className="p-3 border-b border-slate-850 shrink-0">
                <div className="relative">
                  <Search className="absolute left-3 top-2.5 h-3.5 w-3.5 text-slate-500" />
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={`Search ${selectedBucket} memory...`}
                    className="w-full rounded-lg border border-slate-805 bg-slate-955/20 py-2 pl-9 pr-3 text-xs text-slate-300 placeholder-slate-500 outline-none focus:border-primary/60 focus:bg-slate-955/40 transition-all font-sans"
                  />
                </div>
              </div>

              {/* Record List */}
              <div className="flex-1 overflow-y-auto divide-y divide-slate-850/40">
                {loading ? (
                  <div className="space-y-3 p-4">
                    <div className="h-10 bg-slate-800/10 rounded-lg animate-pulse" />
                    <div className="h-10 bg-slate-800/10 rounded-lg animate-pulse" />
                    <div className="h-10 bg-slate-800/10 rounded-lg animate-pulse" />
                  </div>
                ) : filteredRecords.length === 0 ? (
                  <div className="flex flex-col items-center justify-center p-8 text-center h-full">
                    <Database className="h-8 w-8 text-slate-700 mb-3 stroke-1" />
                    <span className="text-xs text-slate-400 font-semibold">No records found</span>
                    <span className="text-[10px] text-slate-500 mt-1">
                      {searchQuery ? "Try resetting search query" : `No persistent ${selectedBucket} memory yet`}
                    </span>
                  </div>
                ) : (
                  filteredRecords.map((record) => {
                    const isSelected = selectedRecord?.id === record.id;
                    return (
                      <button
                        key={record.id}
                        onClick={() => setSelectedRecord(record)}
                        className={`w-full text-left p-3.5 flex flex-col gap-2 transition-all cursor-pointer border-b border-slate-900/40 ${
                          isSelected
                            ? "bg-primary/5 border-l-3 border-primary"
                            : "hover:bg-app-canvas/35"
                        }`}
                      >
                        <div className="flex items-center justify-between w-full">
                          <span className="text-[10px] font-mono text-slate-500 truncate max-w-[120px] font-semibold">
                            {record.id}
                          </span>
                          <span className="text-[10px] font-mono font-bold bg-slate-950/60 text-slate-400 px-2 py-0.5 rounded border border-slate-850">
                            Conf: {(record.confidence * 100).toFixed(0)}%
                          </span>
                        </div>
                        <p className="text-xs text-slate-300 line-clamp-2 leading-relaxed font-sans">
                          {record.content}
                        </p>
                        <div className="flex items-center justify-between text-[10px] text-slate-500 font-mono mt-0.5">
                          <span>Facet: {record.facet || "None"}</span>
                          <span>Scope: {record.scope}</span>
                        </div>
                      </button>
                    );
                  })
                )}
              </div>

              {/* Record Detail Panel */}
              {selectedRecord && (
                <div className="border-t border-slate-850 bg-app-surface/80 p-4.5 flex flex-col gap-3 shrink-0">
                  <div className="flex items-start justify-between">
                    <div className="flex flex-col gap-0.5">
                      <span className="text-[10px] uppercase font-bold text-slate-500 tracking-widest">Selected Memory Detail</span>
                      <span className="text-xs font-mono text-slate-350 font-semibold">{selectedRecord.id}</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => handleCopyRecord(selectedRecord)}
                        className="p-2 rounded-lg bg-slate-950/30 hover:bg-slate-950/65 text-slate-450 hover:text-slate-200 border border-slate-800 transition-all cursor-pointer"
                        title="Copy Content"
                      >
                        {copiedRecordId === selectedRecord.id ? (
                          <Check className="h-4 w-4 text-emerald-400" />
                        ) : (
                          <Copy className="h-4 w-4" />
                        )}
                      </button>
                    </div>
                  </div>

                  <div className="bg-app-canvas/60 border border-slate-850 rounded-xl p-3 text-xs leading-relaxed text-slate-350 font-mono max-h-36 overflow-y-auto whitespace-pre-wrap">
                    {selectedRecord.content}
                  </div>

                  <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-[10px]">
                    <div className="bg-app-canvas/40 p-2.5 rounded-lg border border-slate-850/80">
                      <div className="text-slate-500 font-medium uppercase tracking-wider text-[9px]">Confidence</div>
                      <div className="text-slate-300 font-bold mt-1 font-mono">{(selectedRecord.confidence * 100).toFixed(0)}%</div>
                    </div>
                    <div className="bg-app-canvas/40 p-2.5 rounded-lg border border-slate-850/80">
                      <div className="text-slate-300 font-bold mt-1 font-mono truncate">{selectedRecord.facet || "None"} / {selectedRecord.type}</div>
                    </div>
                    <div className="bg-app-canvas/40 p-2.5 rounded-lg border border-slate-850/80">
                      <div className="text-slate-500 font-medium uppercase tracking-wider text-[9px]">Scope</div>
                      <div className="text-slate-300 font-bold mt-1 font-mono capitalize">{selectedRecord.scope}</div>
                    </div>
                    <div className="bg-app-canvas/40 p-2.5 rounded-lg border border-slate-850/80">
                      <div className="text-slate-500 font-medium uppercase tracking-wider text-[9px]">Created At</div>
                      <div className="text-slate-300 font-bold mt-1 font-mono truncate" title={formatDate(selectedRecord.created_at)}>
                        {formatDate(selectedRecord.created_at)}
                      </div>
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>
        </section>
        )}

        {!isMemoryMode && (
        <section className="flex flex-col h-full overflow-hidden bg-app-canvas">
          <div className="flex items-center justify-between border-b border-slate-850 bg-slate-955/10 px-4 py-2.5 shrink-0">
            <span className="text-[11px] font-bold uppercase tracking-wider text-slate-400 flex items-center gap-2">
              <Cpu className="h-4 w-4 text-primary/80" /> Runtime Context Capsule
            </span>
            <span className="text-[10px] text-slate-500 font-mono">
              State: {snapshot?.context_engine.enabled ? "Active" : "Disabled"}
            </span>
          </div>

          <div className="flex-1 overflow-y-auto p-4 flex flex-col gap-4">
            
            {/* Engine configuration state */}
            {snapshot?.context_engine && (
              <div className="rounded-xl border border-slate-850 bg-slate-950/20 p-4 flex flex-col gap-2.5 shadow-sm">
                <div className="flex items-center justify-between text-xs">
                  <span className="font-bold text-slate-300 flex items-center gap-2">
                    <Sliders className="h-4 w-4 text-slate-500" /> Context Fabric Config
                  </span>
                  <span className={`px-2.5 py-0.5 rounded-md text-[10px] font-bold ${
                    snapshot.context_engine.enabled ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20" : "bg-red-500/10 text-red-400 border border-red-500/20"
                  }`}>
                    {snapshot.context_engine.enabled ? "ENABLED" : "DISABLED"}
                  </span>
                </div>
                {snapshot.context_engine.memory_db && (
                  <div className="text-[10px] text-slate-500 flex items-center gap-1.5 font-mono">
                    <Database className="h-3.5 w-3.5 shrink-0 text-slate-600" />
                    <span className="truncate">DB: {snapshot.context_engine.memory_db}</span>
                  </div>
                )}
                
                {/* Budget Metadata */}
                <div className="grid grid-cols-5 gap-2 mt-1.5">
                  {Object.entries(snapshot.context_engine.budgets).map(([key, value]) => {
                    const label = key.replace("_chars", "");
                    return (
                      <div key={key} className="bg-slate-950/40 p-2 rounded-lg border border-slate-850/80 text-center font-mono">
                        <div className="text-[8px] text-slate-550 uppercase truncate tracking-wider">{label}</div>
                        <div className="text-[10px] font-bold mt-1 text-slate-350">{(value / 1000).toFixed(0)}k</div>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {loading ? (
              <div className="space-y-4">
                <div className="h-12 bg-slate-800/10 rounded-xl animate-pulse" />
                <div className="h-28 bg-slate-800/10 rounded-xl animate-pulse" />
              </div>
            ) : !snapshot?.runtime_context ? (
              <div className="flex flex-col items-center justify-center p-8 text-center flex-1 min-h-[300px]">
                <Cpu className="h-10 w-10 text-slate-700 mb-3 stroke-1 animate-pulse" />
                <h3 className="text-xs font-semibold text-slate-300">No Turn Context Yet</h3>
                <p className="text-[11px] text-slate-500 mt-2 max-w-sm leading-relaxed">
                  No prompt context capsule has been captured in the current session. 
                  Start a conversation in the Chat Workspace to record a context turn.
                </p>
                <div className="mt-4 flex items-center gap-2 text-[10px] text-slate-500 bg-slate-955/30 px-3 py-1.5 rounded-lg border border-slate-850/60 font-mono">
                  <Terminal className="h-3.5 w-3.5 text-slate-655" />
                  <span>Captured in-memory only, lost on daemon restart</span>
                </div>
              </div>
            ) : (
              <div className="flex flex-col gap-4">
                {/* Turn Metadata */}
                <div className="rounded-xl border border-slate-850 bg-slate-955/20 p-4 flex flex-col gap-3 shadow-sm">
                  <div className="flex items-center justify-between border-b border-slate-850/45 pb-2.5">
                    <span className="text-xs font-bold text-slate-200 flex items-center gap-2">
                      <Clock className="h-4 w-4 text-primary" /> Turn Metadata
                    </span>
                    <span className="text-[10px] text-slate-500 font-mono font-medium">{formatDate(snapshot.runtime_context.created_at)}</span>
                  </div>

                  <div className="grid grid-cols-2 md:grid-cols-3 gap-3 text-xs font-mono">
                    <div>
                      <div className="text-slate-500 text-[9px] uppercase tracking-wider font-sans font-bold">Turn ID</div>
                      <div className="text-slate-300 font-bold mt-1 truncate">{snapshot.runtime_context.turn_id}</div>
                    </div>
                    <div>
                      <div className="text-slate-500 text-[9px] uppercase tracking-wider font-sans font-bold">Backend</div>
                      <div className="text-slate-300 font-bold mt-1 capitalize">{snapshot.runtime_context.backend}</div>
                    </div>
                    <div>
                      <div className="text-slate-500 text-[9px] uppercase tracking-wider font-sans font-bold">Model</div>
                      <div className="text-slate-300 font-bold mt-1 truncate">{snapshot.runtime_context.model || "Default"}</div>
                    </div>
                    <div className="col-span-2">
                      <div className="text-slate-500 text-[9px] uppercase tracking-wider font-sans font-bold">Session ID</div>
                      <div className="text-slate-400 font-bold mt-1 truncate">{snapshot.runtime_context.session_id}</div>
                    </div>
                  </div>
                </div>

                {/* Section Summaries */}
                <div className="flex flex-col gap-3">
                  <span className="text-xs font-semibold text-slate-455 uppercase tracking-wider flex items-center gap-2 px-1">
                    <Layers className="h-4 w-4 text-slate-500" /> Capsule Sections ({snapshot.runtime_context.sections.length})
                  </span>

                  {snapshot.runtime_context.sections.length === 0 ? (
                    <div className="text-xs text-slate-500 italic p-3 border border-slate-850 bg-slate-950/10 rounded-xl">
                      No XML context sections parsed from this capsule.
                    </div>
                  ) : (
                    <div className="space-y-2.5">
                      {snapshot.runtime_context.sections.map((section) => (
                        <div
                          key={section.name}
                          className="group rounded-xl border border-slate-850 bg-slate-950/20 p-3.5 hover:bg-app-canvas/40 hover:border-slate-800 transition-all"
                        >
                          <div className="flex items-center justify-between text-xs">
                            <span className="font-mono font-bold text-primary flex items-center gap-1.5">
                              &lt;{section.name}&gt;
                            </span>
                            <span className="text-[10px] text-slate-500 font-mono font-semibold bg-slate-950 px-2.5 py-0.5 rounded border border-slate-850 group-hover:text-slate-300">
                              {section.chars.toLocaleString()} chars
                            </span>
                          </div>
                          {section.preview && (
                            <p className="mt-2.5 text-xs text-slate-400 leading-relaxed bg-app-canvas/55 p-3 rounded-lg border border-slate-900/60 font-mono line-clamp-2">
                              {section.preview}
                            </p>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                {/* Full Capsule text viewer */}
                <div className="rounded-xl border border-slate-850 overflow-hidden bg-slate-955/20 shadow-sm">
                  <button
                    onClick={() => setFullCapsuleExpanded(!fullCapsuleExpanded)}
                    className="w-full flex items-center justify-between bg-slate-950/30 px-4 py-3 hover:bg-slate-955/55 transition-all cursor-pointer"
                  >
                    <span className="text-xs font-bold text-slate-300 flex items-center gap-2">
                      <Terminal className="h-4 w-4 text-slate-500" />
                      Full Raw Context Capsule
                    </span>
                    <span className="text-[10px] text-slate-500 flex items-center gap-1 font-mono font-bold">
                      {fullCapsuleExpanded ? "Collapse" : "Expand"}
                      <ChevronRight className={`h-3.5 w-3.5 transform transition-transform ${fullCapsuleExpanded ? "rotate-90" : ""}`} />
                    </span>
                  </button>

                  {fullCapsuleExpanded && (
                    <div className="border-t border-slate-850 bg-app-canvas rounded-b-xl flex flex-col overflow-hidden">
                      <div className="flex items-center justify-between px-4 py-2 border-b border-slate-850 bg-slate-950/20">
                        <span className="text-[10px] text-slate-550 font-mono">
                          Size: {snapshot.runtime_context.capsule_text.length.toLocaleString()} characters
                        </span>
                        <button
                          onClick={() => handleCopyCapsule(snapshot.runtime_context!.capsule_text)}
                          className="flex items-center gap-1 px-2 py-0.5 rounded bg-slate-950/30 hover:bg-slate-950/60 text-[10px] text-slate-350 border border-slate-850 transition-all cursor-pointer font-mono"
                        >
                          {copiedCapsule ? (
                            <>
                              <Check className="h-3 w-3 text-emerald-400" />
                              <span className="text-emerald-400">Copied</span>
                            </>
                          ) : (
                            <>
                              <Copy className="h-3.5 w-3.5" />
                              <span>Copy Capsule</span>
                            </>
                          )}
                        </button>
                      </div>
                      <pre className="p-4 text-xs text-slate-400 font-mono overflow-auto max-h-72 leading-relaxed whitespace-pre-wrap select-text selection:bg-primary/25 bg-app-canvas/25">
                        {snapshot.runtime_context.capsule_text}
                      </pre>
                    </div>
                  )}
                </div>

              </div>
            )}
          </div>
        </section>
        )}

      </div>
    </div>
  );
}
