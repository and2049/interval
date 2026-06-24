import { Show } from "solid-js";
import { ReplayControls } from "./components/ReplayControls";
import { SessionSelector } from "./components/SessionSelector";
import { SidePanels } from "./components/SidePanels";
import { StintTimeline } from "./components/StintTimeline";
import { TimingTower } from "./components/TimingTower";
import { TrackMap } from "./components/TrackMap";
import { createReplayStore } from "./stores/replay";

export default function App() {
  const replay = createReplayStore();

  return (
    <main class="h-screen min-h-[760px] overflow-hidden bg-carbon text-slate-100">
      <SessionSelector activeSessionKey={replay.sessionKey()} onOpenSession={replay.openSession} />
      <Show
        when={replay.metadata() && replay.snapshot()}
        fallback={
          <div class="flex h-screen items-center justify-center font-mono text-sm text-slate-300">
            Connecting to replay cache...
          </div>
        }
      >
        <ReplayControls
          metadata={replay.metadata()!}
          t={replay.time()}
          playing={replay.playing()}
          speed={replay.speed()}
          onPlayPause={() => replay.setPlaying((value) => !value)}
          onSeek={replay.seek}
          onSpeed={replay.setSpeed}
        />

        <div class="grid h-[calc(100vh-94px)] grid-cols-[42rem_minmax(30rem,1fr)_21rem] grid-rows-[1fr_13rem] gap-2 p-2">
          <TimingTower snapshot={replay.snapshot()!} />
          <TrackMap snapshot={replay.snapshot()!} geometry={replay.trackGeometry()} />
          <SidePanels snapshot={replay.snapshot()!} />
          <div class="col-span-3 min-h-0">
            <StintTimeline snapshot={replay.snapshot()!} />
          </div>
        </div>
      </Show>
    </main>
  );
}
