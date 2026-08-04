import { Settings } from "lucide-solid";
import { createResource, createSignal, Show } from "solid-js";
import type { OpenF1TokenProbe } from "../../../shared/types/api";
import { api } from "../lib/api";
import { badgeClass } from "../lib/replayQuality";
import {
  envOverrideNotice,
  isSubmittableToken,
  probeBadge,
  saveErrorMessage,
  tokenSourceLine
} from "../lib/settingsPanel";

interface SettingsMenuProps {
  /** Re-check live availability once a token has been applied. */
  onTokenApplied?: () => void;
}

export function SettingsMenu(props: SettingsMenuProps) {
  const [open, setOpen] = createSignal(false);
  const [token, setToken] = createSignal("");
  const [reveal, setReveal] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [probe, setProbe] = createSignal<OpenF1TokenProbe>();
  const [error, setError] = createSignal<string>();

  // The settings routes only exist when the desktop shell enables them, so a failure
  // here means "web deployment" and the gear is simply never rendered.
  const [settings, { refetch }] = createResource(() => api.openf1Token().catch(() => null));

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(undefined);
    try {
      await action();
    } catch (failure) {
      setError(saveErrorMessage(failure));
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(async () => {
      setProbe(undefined);
      await api.saveOpenf1Token(token());
      setToken("");
      await refetch();
      props.onTokenApplied?.();
      setProbe(await api.testOpenf1Token());
    });

  const test = () =>
    run(async () => {
      setProbe(await api.testOpenf1Token());
    });

  const clear = () =>
    run(async () => {
      setProbe(undefined);
      await api.clearOpenf1Token();
      await refetch();
      props.onTokenApplied?.();
    });

  return (
    <Show when={settings()}>
      {(current) => (
        <>
          <button
            class="rounded border border-line p-2 text-slate-300 hover:border-mint hover:text-mint"
            title="Settings"
            data-testid="settings-toggle"
            onClick={() => setOpen((value) => !value)}
          >
            <Settings size={13} />
          </button>

          <Show when={open()}>
            <div
              class="absolute right-3 top-full z-10 w-[24rem] border border-line bg-panel p-3 shadow-lg"
              data-testid="settings-panel"
              onKeyDown={(event) => {
                if (event.key === "Escape") setOpen(false);
              }}
            >
              <h2 class="mb-2 font-semibold uppercase tracking-normal text-mint">
                OpenF1 API token
              </h2>

              <p class="mb-2 text-slate-400" data-testid="settings-source">
                {tokenSourceLine(current())}
              </p>

              <div class="mb-2 flex items-center gap-2">
                <input
                  type={reveal() ? "text" : "password"}
                  class="w-full border border-line bg-panel px-2 py-1 text-slate-100 placeholder:text-slate-600"
                  data-testid="settings-token-input"
                  autocomplete="off"
                  spellcheck={false}
                  placeholder={
                    current().configured ? (current().hint ?? "") : "Paste your OpenF1 token"
                  }
                  value={token()}
                  onInput={(event) => setToken(event.currentTarget.value)}
                />
                <button
                  class="border border-line bg-panel px-2 py-1 font-semibold text-slate-300 hover:border-mint hover:text-mint"
                  data-testid="settings-reveal"
                  onClick={() => setReveal((value) => !value)}
                >
                  {reveal() ? "HIDE" : "SHOW"}
                </button>
              </div>

              <div class="mb-2 flex items-center gap-2">
                <button
                  class="border border-mint bg-mint/10 px-3 py-1 font-semibold text-mint disabled:border-line disabled:text-slate-500"
                  data-testid="settings-save"
                  disabled={busy() || !isSubmittableToken(token())}
                  onClick={() => void save()}
                >
                  SAVE
                </button>
                <button
                  class="border border-line bg-panel px-3 py-1 font-semibold text-slate-300 hover:border-mint hover:text-mint disabled:text-slate-600"
                  data-testid="settings-test"
                  disabled={busy() || !current().configured}
                  onClick={() => void test()}
                >
                  TEST
                </button>
                <button
                  class="border border-line bg-panel px-3 py-1 font-semibold text-slate-300 hover:border-mint hover:text-mint disabled:text-slate-600"
                  data-testid="settings-clear"
                  disabled={busy() || current().source !== "settings"}
                  onClick={() => void clear()}
                >
                  CLEAR
                </button>
                <Show when={probe()}>
                  {(result) => (
                    <span
                      class={`border px-2 py-1 ${badgeClass(probeBadge(result()).tone)}`}
                      data-testid="settings-probe-badge"
                    >
                      {probeBadge(result()).label}
                    </span>
                  )}
                </Show>
              </div>

              <Show when={probe()}>
                {(result) => <p class="mb-2 text-slate-400">{result().message}</p>}
              </Show>

              <Show when={error()}>
                {(message) => (
                  <p class="mb-2 text-danger" data-testid="settings-error">
                    {message()}
                  </p>
                )}
              </Show>

              <Show when={envOverrideNotice(current())}>
                {(notice) => <p class="mb-2 text-amber">{notice()}</p>}
              </Show>

              <Show when={current().path}>
                {(path) => (
                  <p class="break-all text-[0.62rem] text-slate-500">Stored in {path()}</p>
                )}
              </Show>
            </div>
          </Show>
        </>
      )}
    </Show>
  );
}
