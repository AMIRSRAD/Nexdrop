import React from 'react';
import {
  FileText,
  FileArchive,
  Film,
  Music,
  AppWindow,
  FileCode,
  File,
  LucideIcon,
} from 'lucide-react';
import { FileCategory } from '../../types/download';

interface FileIconProps {
  category: FileCategory;
  filename: string;
  className?: string;
  size?: number;
}

export const FileIcon: React.FC<FileIconProps> = ({
  category,
  filename,
  className = '',
  size = 20,
}) => {
  let Icon: LucideIcon = File;
  let colorClass = 'text-slate-400 bg-slate-800/60 border-slate-700/50';

  switch (category) {
    case 'videos':
      Icon = Film;
      colorClass = 'text-purple-400 bg-purple-500/10 border-purple-500/20';
      break;
    case 'music':
      Icon = Music;
      colorClass = 'text-pink-400 bg-pink-500/10 border-pink-500/20';
      break;
    case 'compressed':
      Icon = FileArchive;
      colorClass = 'text-amber-400 bg-amber-500/10 border-amber-500/20';
      break;
    case 'documents':
      Icon = FileText;
      colorClass = 'text-blue-400 bg-blue-500/10 border-blue-500/20';
      break;
    case 'programs':
      Icon = AppWindow;
      colorClass = 'text-emerald-400 bg-emerald-500/10 border-emerald-500/20';
      break;
    default:
      if (filename.endsWith('.ts') || filename.endsWith('.js') || filename.endsWith('.rs') || filename.endsWith('.py')) {
        Icon = FileCode;
        colorClass = 'text-cyan-400 bg-cyan-500/10 border-cyan-500/20';
      }
      break;
  }

  return (
    <div
      className={`flex items-center justify-center rounded-xl p-2.5 border transition-transform duration-200 group-hover:scale-105 ${colorClass} ${className}`}
    >
      <Icon size={size} strokeWidth={1.8} />
    </div>
  );
};
