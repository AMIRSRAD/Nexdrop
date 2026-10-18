import React from 'react';
import { Magnet } from 'lucide-react';
import { categoryMeta } from '../../lib/meta';
import { cn } from '../../lib/cn';

export const FileTile: React.FC<{ category: string; size?: number; className?: string; torrent?: boolean }> = React.memo(
  ({ category, size = 32, className, torrent }) => {
    const meta = torrent ? { icon: Magnet, color: 'var(--accent)' } : categoryMeta(category);
    const Icon = meta.icon;
    return (
      <div
        className={cn('flex shrink-0 items-center justify-center rounded-lg', className)}
        style={{
          width: size,
          height: size,
          color: meta.color,
          background: `color-mix(in srgb, ${meta.color} 13%, transparent)`,
        }}
      >
        <Icon size={Math.round(size * 0.5)} strokeWidth={1.8} />
      </div>
    );
  },
);
FileTile.displayName = 'FileTile';
