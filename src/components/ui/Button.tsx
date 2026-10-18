import React from 'react';
import type { LucideIcon } from 'lucide-react';
import { LoaderCircle } from 'lucide-react';
import { cn } from '../../lib/cn';
import { Tooltip } from './Tooltip';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'subtle';
type Size = 'sm' | 'md' | 'lg';

const variants: Record<Variant, string> = {
  primary:
    'bg-accent text-accent-fg hover:bg-accent-hover shadow-[inset_0_1px_0_rgba(255,255,255,0.15)] disabled:hover:bg-accent',
  secondary:
    'bg-surface-2 text-fg border border-line-strong hover:bg-surface-3 disabled:hover:bg-surface-2',
  subtle: 'bg-surface-2 text-fg hover:bg-surface-3',
  ghost: 'text-fg-2 hover:text-fg hover:bg-hover active:bg-press',
  danger: 'bg-danger text-white hover:brightness-110',
};

const sizes: Record<Size, string> = {
  sm: 'h-7 px-2.5 text-[12px] gap-1.5 rounded-md',
  md: 'h-8 px-3 text-[13px] gap-2 rounded-lg',
  lg: 'h-9 px-4 text-[13px] gap-2 rounded-lg',
};

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: LucideIcon;
  loading?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'secondary', size = 'md', icon: Icon, loading, className, children, disabled, type = 'button', ...rest }, ref) => (
    <button
      ref={ref}
      type={type}
      disabled={disabled || loading}
      className={cn(
        'inline-flex shrink-0 items-center justify-center font-medium whitespace-nowrap transition-[background-color,color,filter,opacity] duration-100 select-none disabled:opacity-45',
        variants[variant],
        sizes[size],
        className,
      )}
      {...rest}
    >
      {loading ? (
        <LoaderCircle size={14} className="animate-spin" />
      ) : Icon ? (
        <Icon size={size === 'sm' ? 13 : 15} strokeWidth={2} />
      ) : null}
      {children}
    </button>
  ),
);
Button.displayName = 'Button';

export interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon: LucideIcon;
  label: string;
  shortcut?: string;
  size?: 'sm' | 'md';
  active?: boolean;
  tone?: 'default' | 'danger' | 'accent';
}

export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ icon: Icon, label, shortcut, size = 'md', active, tone = 'default', className, type = 'button', ...rest }, ref) => (
    <Tooltip content={label} shortcut={shortcut}>
      <button
        ref={ref}
        type={type}
        aria-label={label}
        className={cn(
          'inline-flex shrink-0 items-center justify-center rounded-md text-fg-2 transition-colors duration-100 hover:bg-hover hover:text-fg active:bg-press disabled:opacity-40 disabled:hover:bg-transparent',
          size === 'sm' ? 'h-7 w-7' : 'h-8 w-8',
          active && 'bg-press text-fg',
          tone === 'danger' && 'hover:text-danger',
          tone === 'accent' && 'text-accent hover:text-accent',
          className,
        )}
        {...rest}
      >
        <Icon size={size === 'sm' ? 15 : 16} strokeWidth={1.9} />
      </button>
    </Tooltip>
  ),
);
IconButton.displayName = 'IconButton';

export const Kbd: React.FC<{ children: React.ReactNode; className?: string }> = ({ children, className }) => (
  <kbd
    className={cn(
      'inline-flex h-[18px] min-w-[18px] items-center justify-center rounded border border-line-strong bg-surface-2 px-1 font-sans text-[10.5px] font-medium text-fg-3',
      className,
    )}
  >
    {children}
  </kbd>
);
