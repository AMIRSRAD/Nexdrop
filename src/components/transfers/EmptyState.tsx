import React from 'react';
import { ListPlus, Plus, SearchX } from 'lucide-react';
import { Button, Kbd } from '../ui/Button';

export const EmptyState: React.FC<{
  variant: 'empty' | 'filtered';
  onAdd: () => void;
  onAddMultiple: () => void;
  onClearFilters: () => void;
  filterLabel?: string;
}> = ({ variant, onAdd, onAddMultiple, onClearFilters, filterLabel }) => {
  if (variant === 'filtered') {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 pb-16 text-center">
        <SearchX size={28} className="text-fg-3" strokeWidth={1.6} />
        <div>
          <div className="text-[14px] font-medium text-fg">Nothing here</div>
          <div className="mt-1 text-[12.5px] text-fg-3">{filterLabel ?? 'No downloads match the current filters.'}</div>
        </div>
        <Button size="sm" variant="secondary" onClick={onClearFilters}>
          Show all downloads
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col items-center justify-center pb-16 text-center">
      <DropIllustration />
      <h2 className="mt-6 text-[17px] font-semibold tracking-[-0.01em] text-fg">Ready when you are</h2>
      <p className="mt-1.5 max-w-[340px] text-[13px] leading-relaxed text-fg-2">
        Paste a link into the bar above, drag one onto this window, or send it from your browser.
      </p>
      <div className="mt-6 flex items-center gap-2">
        <Button variant="primary" icon={Plus} onClick={onAdd}>
          New download
        </Button>
        <Button variant="secondary" icon={ListPlus} onClick={onAddMultiple}>
          Add multiple
        </Button>
      </div>
      <div className="mt-8 flex items-center gap-5 text-[12px] text-fg-3">
        <span className="flex items-center gap-1.5">
          <Kbd>Ctrl</Kbd>
          <Kbd>V</Kbd> paste a link
        </span>
        <span className="flex items-center gap-1.5">
          <Kbd>Ctrl</Kbd>
          <Kbd>N</Kbd> new download
        </span>
        <span className="flex items-center gap-1.5">
          <Kbd>Ctrl</Kbd>
          <Kbd>B</Kbd> add multiple
        </span>
      </div>
    </div>
  );
};

const DropIllustration: React.FC = () => (
  <svg width="120" height="96" viewBox="0 0 120 96" fill="none" aria-hidden>
    <rect x="18" y="26" width="84" height="58" rx="12" fill="var(--surface-2)" stroke="var(--line-strong)" />
    <rect x="30" y="40" width="44" height="5" rx="2.5" fill="var(--track)" />
    <rect x="30" y="40" width="28" height="5" rx="2.5" fill="var(--accent)" opacity=".9" />
    <rect x="30" y="54" width="60" height="5" rx="2.5" fill="var(--track)" />
    <rect x="30" y="54" width="60" height="5" rx="2.5" fill="var(--success)" opacity=".75" />
    <rect x="30" y="68" width="36" height="5" rx="2.5" fill="var(--track)" />
    <circle cx="88" cy="22" r="16" fill="var(--accent)" />
    <path d="M88 14v13m0 0-5-5m5 5 5-5" stroke="#fff" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
