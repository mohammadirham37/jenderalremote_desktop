// Komponen kecil bersama: tombol, chip status, dan timeline koneksi.

import type { ReactNode } from "react";

type ButtonVariant = "primary" | "ghost" | "danger";

export function Button({
  variant = "primary",
  children,
  className = "",
  ...rest
}: {
  variant?: ButtonVariant;
  children: ReactNode;
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const base =
    "rounded-lg px-4 py-2.5 text-sm font-semibold transition-[filter,background-color,border-color] disabled:cursor-not-allowed disabled:opacity-40 active:brightness-95";
  const look: Record<ButtonVariant, string> = {
    primary:
      "bg-brass text-brass-ink shadow-[0_2px_14px_rgba(227,168,87,0.25)] hover:brightness-110",
    ghost:
      "border border-line-strong text-ink hover:border-brass/60 hover:bg-panel-raised",
    danger:
      "border border-danger/50 text-danger hover:bg-danger-dim/60",
  };
  return (
    <button className={`${base} ${look[variant]} ${className}`} {...rest}>
      {children}
    </button>
  );
}

export function StatChip({ label, value, tone = "mono" }: { label: string; value: string; tone?: "mono" | "live" }) {
  return (
    <div className="rounded-lg border border-line bg-base/70 px-3 py-2">
      <div className="text-[11px] text-muted">{label}</div>
      <div
        className={`text-sm ${
          tone === "live" ? "text-phosphor" : "font-mono text-brass"
        }`}
      >
        {value}
      </div>
    </div>
  );
}

// Langkah koneksi — urutan nyata, jadi penanda langkah memang tepat.
const STEPS = ["Permintaan", "Persetujuan", "Negosiasi", "Aktif"];

export function stepIndex(status: string): number {
  switch (status) {
    case "idle":
    case "requesting":
      return 0;
    case "awaiting_approval":
      return 1;
    case "negotiating":
    case "connecting":
      return 2;
    case "active":
      return 3;
    default:
      return -1;
  }
}

export function ConnectionSteps({ status }: { status: string }) {
  const current = stepIndex(status);
  const failed = ["rejected", "failed", "revoked", "ended", "disconnected"].includes(status);
  return (
    <ol className="flex items-center gap-1.5">
      {STEPS.map((step, index) => {
        const done = current >= index && current !== -1;
        return (
          <li key={step} className="flex min-w-0 items-center gap-1.5">
            {index > 0 && (
              <span
                className={`h-px w-4 shrink-0 ${current >= index ? "bg-phosphor/70" : "bg-line"}`}
              />
            )}
            <span
              className={`flex h-5 items-center gap-1.5 rounded-full border px-2 text-[11px] ${
                failed && index === Math.max(current, 0)
                  ? "border-danger/60 text-danger"
                  : done
                    ? "border-phosphor/50 text-phosphor"
                    : "border-line text-muted"
              }`}
            >
              <span
                className={`h-1.5 w-1.5 rounded-full ${
                  failed && index === Math.max(current, 0)
                    ? "bg-danger"
                    : done
                      ? "bg-phosphor"
                      : "bg-line-strong"
                } ${done && index === current && !failed ? "animate-pulse" : ""}`}
              />
              <span className="truncate">{step}</span>
            </span>
          </li>
        );
      })}
    </ol>
  );
}
