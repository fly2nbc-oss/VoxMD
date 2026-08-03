interface Props {
  /** Radio group name; must be unique on the page. */
  name: string;
  value: string;
  /** Value written when the non-ISO option is picked (`system` or `auto`). */
  defaultValue: string;
  defaultLabel: string;
  isDefault: (value: string) => boolean;
  /** Shown beside the default option, e.g. the detected system language. */
  detected?: string;
  /** Seed for the ISO field when switching away from the default. */
  isoFallback: () => string;
  isoAriaLabel: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}

/**
 * Shared by the summary and transcription language settings, which differ only
 * in their default option and the fallback used when switching to an ISO code.
 */
export function LanguagePicker({
  name,
  value,
  defaultValue,
  defaultLabel,
  isDefault,
  detected,
  isoFallback,
  isoAriaLabel,
  disabled = false,
  onChange,
}: Props) {
  const usingDefault = isDefault(value);
  const toIso = () => onChange(isoFallback() || "de");

  return (
    <div className="lang-option-row">
      <label className="lang-radio">
        <input
          type="radio"
          name={name}
          checked={usingDefault}
          disabled={disabled}
          onChange={() => onChange(defaultValue)}
        />
        <span>{defaultLabel}</span>
        {usingDefault && detected ? <span className="lang-detected">({detected})</span> : null}
      </label>
      <label className="lang-radio">
        <input
          type="radio"
          name={name}
          checked={!usingDefault}
          disabled={disabled}
          onChange={toIso}
        />
        <span>ISO code</span>
      </label>
      <input
        className="input lang-iso-input"
        aria-label={isoAriaLabel}
        placeholder="de"
        disabled={disabled}
        value={usingDefault ? "" : value}
        onChange={(e) => onChange(e.target.value || "de")}
        // Typing into the field implies the ISO mode, so switch on focus.
        onFocus={() => {
          if (usingDefault) toIso();
        }}
      />
    </div>
  );
}
