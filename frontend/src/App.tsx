import { createSignal, Show } from "solid-js";
import { ReplayControls } from "./components/ReplayControls";
import { SessionSelector } from "./components/SessionSelector";
import { SidePanels } from "./components/SidePanels";
import { StintTimeline } from "./components/StintTimeline";
import { TimingTower } from "./components/TimingTower";
import { TrackMap } from "./components/TrackMap";
import { replayLoadMessage } from "./lib/replayPlayback";
import { createReplayStore } from "./stores/replay";

export default function App() {
  const replay = createReplayStore();
  const metadata = () => replay.displayMetadata();
  const snapshot = () => replay.activeSnapshot();
  const [selectedSessionLabel, setSelectedSessionLabel] = createSignal<string>();

  return (
    <main class="h-screen overflow-hidden bg-carbon text-slate-100">
      <SessionSelector
        activeSession={metadata()?.session}
        activeSessionKey={replay.sessionKey()}
        onOpenSession={replay.openSession}
        onSessionIntent={replay.clearActiveSession}
        onSelectionChange={(selection) => setSelectedSessionLabel(selection.label)}
        liveStatusMessage={replay.liveAvailabilityMessage()}
        liveChecking={replay.liveAvailabilityChecking()}
        liveActive={replay.liveActive()}
        onCheckLive={replay.checkLive}
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
                selectedSessionLabel: selectedSessionLabel(),
                liveStatusMessage: replay.liveAvailabilityMessage()
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
          liveSimulationActive={replay.liveSimulationActive()}
          liveSimulationStatus={replay.liveSimulationConnection()}
          liveActive={replay.liveActive()}
          liveStatus={replay.liveConnection()}
          liveRuntimeStatus={replay.liveStatus()}
          liveChecking={replay.liveAvailabilityChecking()}
          liveChannels={replay.liveChannels()}
          onPlayPause={() => replay.setPlaying((value) => !value)}
          onSeek={replay.seek}
          onSpeed={replay.setSpeed}
          onLiveCheck={replay.checkLive}
          onLiveStop={() => void replay.stopLive()}
          onLiveSimulationToggle={() => void replay.toggleLiveSimulation()}
        />

        <div class="grid h-[calc(100vh-105px)] grid-cols-[minmax(31rem,38rem)_minmax(26rem,1fr)_minmax(18rem,21rem)] grid-rows-[1fr_13rem] gap-2 p-2">
          <TimingTower snapshot={snapshot()!} />
          <TrackMap
            snapshot={snapshot()!}
            geometry={replay.activeGeometry()}
            geometryError={replay.activeGeometryError()}
            frameStepSeconds={metadata()!.frame_step_seconds}
            playing={replay.playing() || replay.liveActive() || replay.liveSimulationActive()}
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
