import { Show } from "solid-js";
import { ReplayControls } from "./components/ReplayControls";
import { SessionSelector } from "./components/SessionSelector";
import { SidePanels } from "./components/SidePanels";
import { StintTimeline } from "./components/StintTimeline";
import { TimingTower } from "./components/TimingTower";
import { TrackMap } from "./components/TrackMap";
import { replayLoadMessage } from "./lib/replayPlayback";
import {
  MVP_HISTORICAL_MEETING_KEY,
  MVP_HISTORICAL_SEASON,
  MVP_HISTORICAL_SESSION_KEY
} from "./lib/sessionKeys";
import { createReplayStore } from "./stores/replay";

export default function App() {
  const replay = createReplayStore();
  const metadata = () => replay.activeMetadata();
  const snapshot = () => replay.activeSnapshot();

  return (
    <main class="h-screen overflow-hidden bg-carbon text-slate-100">
      <SessionSelector
        activeSession={metadata()?.session}
        activeSessionKey={replay.sessionKey()}
        preferredSeason={MVP_HISTORICAL_SEASON}
        preferredMeeting={MVP_HISTORICAL_MEETING_KEY}
        preferredSession={MVP_HISTORICAL_SESSION_KEY}
        onOpenSession={replay.openSession}
      />
      <Show
        when={metadata() && snapshot()}
        fallback={
          <div class="flex h-screen items-center justify-center px-6 font-mono text-sm text-slate-300">
            <div class="border border-line bg-panel px-4 py-3">
              {replayLoadMessage({
                metadata: metadata(),
                metadataLoading: replay.metadata.loading,
                metadataError: replay.metadata.error,
                snapshotLoading: replay.snapshot.loading,
                snapshotError: replay.snapshot.error,
                sessionKey: replay.sessionKey(),
                preferredHistoricalSessionKey: MVP_HISTORICAL_SESSION_KEY
              })}
            </div>
          </div>
        }
      >
        <ReplayControls
          metadata={metadata()!}
          t={replay.time()}
          playing={replay.playing()}
          speed={replay.speed()}
          onPlayPause={() => replay.setPlaying((value) => !value)}
          onSeek={replay.seek}
          onSpeed={replay.setSpeed}
        />

        <div class="grid h-[calc(100vh-105px)] grid-cols-[minmax(31rem,38rem)_minmax(26rem,1fr)_minmax(18rem,21rem)] grid-rows-[1fr_13rem] gap-2 p-2">
          <TimingTower snapshot={snapshot()!} />
          <TrackMap
            snapshot={snapshot()!}
            geometry={replay.activeGeometry()}
            geometryError={replay.activeGeometryError()}
            frameStepSeconds={metadata()!.frame_step_seconds}
            playing={replay.playing()}
            speed={replay.speed()}
          />
          <SidePanels
            snapshot={snapshot()!}
            events={replay.activeEvents()}
            eventsLoading={replay.activeEventsLoading()}
            eventsError={replay.activeEventsError()}
          />
          <div class="col-span-3 min-h-0">
            <StintTimeline snapshot={snapshot()!} />
          </div>
        </div>
      </Show>
    </main>
  );
}
