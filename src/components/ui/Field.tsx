import React from 'react';
import { Check, ChevronDown, type LucideIcon } from 'lucide-react';
import { cn } from '../../lib/cn';

const controlBase =
  'w-full rounded-lg border border-line-strong bg-surface-2 text-[13px] text-fg transition-[border-color,box-shadow] duration-100 outline-none hover:border-[color:var(--fg-3)]/40 focus:border-accent focus:shadow-[0_0_0_3px_var(--accent-soft)] disabled:opacity-50';

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  icon?: LucideIcon;
  suffix?: React.ReactNode;
  mono?: boolean;
  inputSize?: 'md' | 'lg';
}

export const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ icon: Icon, suffix, mono, inputSize = 'md', className, ...rest }, ref) => (
    <div className={cn('relative flex items-center', className)}>
      {Icon && <Icon size={15} className="pointer-events-none absolute left-2.5 text-fg-3" strokeWidth={1.9} />}
      <input
        ref={ref}
        spellCheck={false}
        className={cn(
          controlBase,
          inputSize === 'lg' ? 'h-10' : 'h-8',
          Icon ? 'pl-8' : 'pl-2.5',
          suffix ? 'pr-20' : 'pr-2.5',
          mono && 'font-mono text-[12px]',
        )}
        {...rest}
      />
      {suffix && <div className="absolute right-1.5 flex items-center gap-1">{suffix}</div>}
    </div>
  ),
);
Input.displayName = 'Input';

export const Textarea = React.forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement> & { mono?: boolean }>(
  ({ className, mono, ...rest }, ref) => (
    <textarea
      ref={ref}
      spellCheck={false}
      className={cn(controlBase, 'resize-none px-2.5 py-2 leading-relaxed', mono && 'font-mono text-[12px]', className)}
      {...rest}
    />
  ),
);
Textarea.displayName = 'Textarea';

export interface SelectProps extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'onChange'> {
  options: { value: string | number; label: string }[];
  onValueChange: (value: string) => void;
}

export const Select: React.FC<SelectProps> = ({ options, onValueChange, className, ...rest }) => (
  <div className={cn('relative', className)}>
    <select
      onChange={(e) => onValueChange(e.target.value)}
      className={cn(controlBase, 'h-8 appearance-none pr-8 pl-2.5')}
      {...rest}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value} className="bg-elevated text-fg">
          {o.label}
        </option>
      ))}
    </select>
    <ChevronDown size={14} className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-fg-3" />
  </div>
);

export const Label: React.FC<{ children: React.ReactNode; hint?: React.ReactNode; htmlFor?: string; className?: string }> = ({
  children,
  hint,
  htmlFor,
  className,
}) => (
  <div className={cn('mb-1.5 flex items-baseline justify-between gap-3', className)}>
    <label htmlFor={htmlFor} className="text-[12px] font-medium text-fg-2">
      {children}
    </label>
    {hint && <span className="text-[11.5px] text-fg-3">{hint}</span>}
  </div>
);

export const Switch: React.FC<{
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  disabled?: boolean;
}> = ({ checked, onChange, label, disabled }) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    disabled={disabled}
    onClick={() => onChange(!checked)}
    className={cn(
      'relative inline-flex h-[20px] w-[34px] shrink-0 items-center rounded-full transition-colors duration-150 disabled:opacity-40',
      checked ? 'bg-accent' : 'bg-surface-3 shadow-[inset_0_0_0_1px_var(--line-strong)]',
    )}
  >
    <span
      className={cn(
        'h-[14px] w-[14px] rounded-full bg-white shadow-sm transition-transform duration-150',
        checked ? 'translate-x-[17px]' : 'translate-x-[3px]',
      )}
    />
  </button>
);

export const Checkbox: React.FC<{
  checked: boolean;
  indeterminate?: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  className?: string;
}> = ({ checked, indeterminate, onChange, label, className }) => (
  <button
    type="button"
    role="checkbox"
    aria-checked={indeterminate ? 'mixed' : checked}
    aria-label={label}
    onClick={(e) => {
      e.stopPropagation();
      onChange(!checked);
    }}
    onDoubleClick={(e) => e.stopPropagation()}
    className={cn(
      'flex h-4 w-4 shrink-0 items-center justify-center rounded-[4px] border transition-colors duration-100',
      checked || indeterminate
        ? 'border-accent bg-accent text-accent-fg'
        : 'border-line-strong bg-surface-2 hover:border-fg-3',
      className,
    )}
  >
    {indeterminate ? (
      <span className="h-[2px] w-2 rounded bg-current" />
    ) : checked ? (
      <Check size={11} strokeWidth={3.2} />
    ) : null}
  </button>
);

export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  size = 'md',
  className,
}: {
  value: T;
  options: { value: T; label: React.ReactNode; icon?: LucideIcon }[];
  onChange: (value: T) => void;
  size?: 'sm' | 'md';
  className?: string;
}) {
  return (
    <div
      role="radiogroup"
      className={cn('inline-flex rounded-lg bg-surface-2 p-0.5 shadow-[inset_0_0_0_1px_var(--line)]', className)}
    >
      {options.map((o) => {
        const active = o.value === value;
        const Icon = o.icon;
        return (
          <button
            key={String(o.value)}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(o.value)}
            className={cn(
              'inline-flex flex-1 items-center justify-center gap-1.5 rounded-md font-medium whitespace-nowrap transition-colors duration-100',
              size === 'sm' ? 'h-6 px-2.5 text-[12px]' : 'h-7 px-3 text-[12.5px]',
              active ? 'bg-elevated text-fg shadow-pop' : 'text-fg-2 hover:text-fg',
            )}
          >
            {Icon && <Icon size={13} />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** Number input with unit suffix, e.g. "512 KB/s". */
export const NumberInput: React.FC<{
  value: number;
  onChange: (n: number) => void;
  min?: number;
  max?: number;
  unit?: string;
  placeholder?: string;
  className?: string;
}> = ({ value, onChange, min = 0, max, unit, placeholder, className }) => (
  <div className={cn('relative', className)}>
    <input
      type="number"
      min={min}
      max={max}
      value={Number.isFinite(value) && value !== 0 ? value : value === 0 && placeholder ? '' : value}
      placeholder={placeholder}
      onChange={(e) => {
        let n = parseInt(e.target.value, 10);
        if (isNaN(n)) n = min;
        if (max !== undefined) n = Math.min(max, n);
        onChange(Math.max(min, n));
      }}
      className={cn(controlBase, 'tabular h-8 pl-2.5', unit ? 'pr-14' : 'pr-2.5')}
    />
    {unit && (
      <span className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-[12px] text-fg-3">{unit}</span>
    )}
  </div>
);
