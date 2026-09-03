import type { FieldDefinition } from "../../contracts";
import { editableValue } from "./tableModel";

export function CustomFieldInput({
  field,
  value,
  disabled,
  onChange,
}: {
  field: FieldDefinition;
  value: unknown;
  disabled: boolean;
  onChange: (value: unknown) => void;
}) {
  if (field.fieldType === "boolean") {
    return (
      <select
        aria-label={field.displayName}
        disabled={disabled}
        value={typeof value === "boolean" ? String(value) : ""}
        onChange={(event) =>
          onChange(
            event.target.value === ""
              ? undefined
              : event.target.value === "true",
          )
        }
      >
        <option value="">未填写</option>
        <option value="true">是</option>
        <option value="false">否</option>
      </select>
    );
  }
  if (field.fieldType === "select") {
    const config = field.config;
    const options =
      config &&
      typeof config === "object" &&
      "options" in config &&
      Array.isArray(config.options)
        ? config.options.filter(
            (option): option is string => typeof option === "string",
          )
        : [];
    return (
      <select
        aria-label={field.displayName}
        disabled={disabled}
        value={String(editableValue(value))}
        onChange={(event) => onChange(event.target.value || undefined)}
      >
        <option value="">未填写</option>
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  }
  return (
    <input
      aria-label={field.displayName}
      disabled={disabled}
      value={editableValue(value)}
      type={
        field.fieldType === "date"
          ? "date"
          : field.fieldType === "number"
            ? "number"
            : field.fieldType === "url"
              ? "url"
              : "text"
      }
      onChange={(event) =>
        onChange(
          event.target.value === ""
            ? undefined
            : field.fieldType === "number"
              ? Number(event.target.value)
              : event.target.value,
        )
      }
    />
  );
}
