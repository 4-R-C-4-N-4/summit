import PulseDot from '../common/PulseDot';

export default function TransferCard({ filename }) {
  return (
    <div className="bg-summit-accent/5 border border-summit-accent/15 rounded-lg px-3 py-2.5 flex items-center gap-2.5">
      <PulseDot color="#58a6ff" size={5} />
      <span className="text-[11px] text-summit-accent truncate">{filename}</span>
    </div>
  );
}
