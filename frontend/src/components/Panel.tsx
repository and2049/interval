import type { JSX } from "solid-js";

interface PanelProps {
  title: string;
  action?: JSX.Element;
  children: JSX.Element;
  class?: string;
}

export function Panel(props: PanelProps) {
  return (
    <section class={`panel min-h-0 overflow-hidden ${props.class ?? ""}`}>
      <header class="flex h-8 items-center justify-between border-b border-line bg-panelHi px-2">
        <h2 class="font-mono text-[0.72rem] font-semibold uppercase tracking-normal text-mint">
          {props.title}
        </h2>
        {props.action}
      </header>
      <div class="min-h-0">{props.children}</div>
    </section>
  );
}
