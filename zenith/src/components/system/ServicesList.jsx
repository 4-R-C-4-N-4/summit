import { useDaemon } from '../../hooks/useDaemon';
import SectionHeader from '../common/SectionHeader';
import Badge from '../common/Badge';

export default function ServicesList() {
  const { services } = useDaemon();
  const list = services || [];

  return (
    <div>
      <SectionHeader label="Services" count={list.length} accent="text-summit-purple" />
      {list.length === 0 ? (
        <div className="bg-summit-raised/30 border border-summit-border rounded-xl py-6 text-center">
          <div className="text-[11px] text-white/20">No services registered</div>
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {list.map((svc, i) => (
            <div
              key={typeof svc === 'string' ? svc : svc.name || i}
              className="bg-summit-raised/40 border border-summit-border rounded-lg px-4 py-2.5 flex items-center justify-between"
            >
              <span className="text-[11px] text-summit-purple font-bold">
                {typeof svc === 'string' ? svc : svc.name || 'unknown'}
              </span>
              <Badge variant="trusted" label="active" />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
