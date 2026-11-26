interface FluentSwitchProps {
  checked: boolean;
  disabled?: boolean;
  label: string;
  onToggle: () => void;
}

export function FluentSwitch({
  checked,
  disabled = false,
  label,
  onToggle,
}: FluentSwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={onToggle}
      className={`group w-10 h-5 rounded-full border transition-all duration-200 ease-out relative flex items-center shrink-0 disabled:opacity-50 disabled:cursor-not-allowed ${
        checked
          ? "bg-accent border-accent"
          : "bg-switch-off-bg border-switch-off-border hover:border-text-primary/70"
      }`}
    >
      <span
        className={`block w-3 h-3 rounded-full origin-center transition-transform duration-200 ease-out group-active:scale-x-[1.18] ${
          checked
            ? "translate-x-[22px] bg-accent-fg"
            : "translate-x-[3px] bg-text-secondary"
        }`}
      />
    </button>
  );
}
