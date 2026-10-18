import { useEffect, useRef, useState } from 'react';

const SAMPLES = 90;

/** Samples the aggregate speed once per second for sparklines & stats. */
export function useSpeedHistory(currentSpeed: number) {
  const latest = useRef(currentSpeed);
  latest.current = currentSpeed;
  const [history, setHistory] = useState<number[]>(() => Array(SAMPLES).fill(0));
  const [peak, setPeak] = useState(0);

  useEffect(() => {
    const id = window.setInterval(() => {
      const v = latest.current;
      setHistory((prev) => (prev[prev.length - 1] === 0 && v === 0 && prev.every((x) => x === 0) ? prev : [...prev.slice(1), v]));
      setPeak((p) => (v > p ? v : p));
    }, 1000);
    return () => window.clearInterval(id);
  }, []);

  return { history, peak };
}
