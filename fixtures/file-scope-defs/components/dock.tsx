import TideChart from "./tideChart";
import AnchorButton from "./anchorButton";
import BuoyBadge from "./buoyBadge";
import { HarborBanner } from "./harborBanner";

export default function DockPanel() {
  return (
    <main>
      <TideChart values={[1, 2]} />
      <AnchorButton label="Open" />
      <BuoyBadge />
      <HarborBanner tone="calm" />
    </main>
  );
}
