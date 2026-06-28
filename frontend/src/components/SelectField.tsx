import { createEffect, For } from "solid-js";
import { parseSelectNumber } from "../lib/selectField";

export interface SelectOption {
  value: number;
  label: string;
  disabled?: boolean;
  title?: string;
}

interface SelectFieldProps {
  label: string;
  testId: string;
  value?: number;
  options: SelectOption[];
  disabled?: boolean;
  class?: string;
  labelClass?: string;
  onChange: (value: number) => void;
}

export function SelectField(props: SelectFieldProps) {
  let selectRef: HTMLSelectElement | undefined;

  createEffect(() => {
    if (!selectRef) return;
    props.options.length;
    selectRef.value = props.value?.toString() ?? "";
  });

  return (
    <>
      <label class={props.labelClass ?? "text-slate-500"}>{props.label}</label>
      <select
        ref={selectRef}
        class={props.class ?? "border border-line bg-panel px-2 py-1 text-slate-100"}
        data-testid={props.testId}
        value={props.value?.toString() ?? ""}
        disabled={props.disabled}
        onChange={(event) => {
          const next = parseSelectNumber(event.currentTarget.value);
          if (next !== undefined) props.onChange(next);
        }}
      >
        {props.value === undefined && <option value="" disabled>Select {props.label}</option>}
        <For each={props.options}>
          {(option) => (
            <option value={option.value} disabled={option.disabled} title={option.title}>
              {option.label}
            </option>
          )}
        </For>
      </select>
    </>
  );
}
