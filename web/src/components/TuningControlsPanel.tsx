import React from 'react';
import {
  DEFAULT_SYNTHESIS_TUNING,
  SYNTHESIS_TUNING_PRESETS,
  type SynthesisTuning,
} from '../types/api';

export interface TuningControlsPanelProps {
  value: SynthesisTuning;
  onChange: (next: SynthesisTuning) => void;
  idPrefix: string;
  compact?: boolean;
}

function matchesPreset(current: SynthesisTuning, preset: SynthesisTuning): boolean {
  return (
    Math.abs(current.temperature - preset.temperature) < 0.001 &&
    Math.abs(current.top_p - preset.top_p) < 0.001 &&
    current.top_k === preset.top_k &&
    Math.abs(current.repetition_penalty - preset.repetition_penalty) < 0.01 &&
    Math.abs(current.speed - preset.speed) < 0.01 &&
    current.gpt_cond_len_secs === preset.gpt_cond_len_secs &&
    current.gpt_cond_chunk_len_secs === preset.gpt_cond_chunk_len_secs &&
    current.sound_norm_refs === preset.sound_norm_refs
  );
}

export const TuningControlsPanel: React.FC<TuningControlsPanelProps> = ({
  value,
  onChange,
  idPrefix,
  compact = false,
}) => {
  const updateField = <K extends keyof SynthesisTuning>(
    key: K,
    fieldVal: SynthesisTuning[K],
  ) => {
    onChange({
      ...value,
      [key]: fieldVal,
    });
  };

  return (
    <div
      className={`tuning-panel ${compact ? 'tuning-panel-compact' : ''}`}
      data-testid={`${idPrefix}-panel`}
    >
      <div className="tuning-presets-bar">
        <span className="script-label">Quick Likeness Presets:</span>
        <div className="inline-actions">
          {SYNTHESIS_TUNING_PRESETS.map((preset) => {
            const active = matchesPreset(value, preset.tuning);
            return (
              <button
                key={preset.id}
                type="button"
                title={preset.description}
                className={`btn btn-sm ${active ? 'btn-primary' : 'btn-secondary'}`}
                onClick={() => onChange({ ...preset.tuning })}
              >
                {preset.label}
              </button>
            );
          })}
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => onChange({ ...DEFAULT_SYNTHESIS_TUNING })}
          >
            Reset Defaults
          </button>
        </div>
      </div>

      <div className="tuning-sliders-grid">
        <div className="slider-field">
          <div className="label-row">
            <label htmlFor={`${idPrefix}-temperature`}>Temperature</label>
            <span className="mono badge badge-neutral">
              {value.temperature.toFixed(2)}
            </span>
          </div>
          <input
            id={`${idPrefix}-temperature`}
            type="range"
            min={0.05}
            max={1.2}
            step={0.01}
            value={value.temperature}
            onChange={(e) => updateField('temperature', Number(e.target.value))}
          />
          <div className="field-hint">
            0.05–1.20 · Lower = closer to your exact vocal timbre; higher = more varied inflection.
          </div>
        </div>

        <div className="slider-field">
          <div className="label-row">
            <label htmlFor={`${idPrefix}-top-p`}>Top-P</label>
            <span className="mono badge badge-neutral">{value.top_p.toFixed(2)}</span>
          </div>
          <input
            id={`${idPrefix}-top-p`}
            type="range"
            min={0.1}
            max={1.0}
            step={0.02}
            value={value.top_p}
            onChange={(e) => updateField('top_p', Number(e.target.value))}
          />
          <div className="field-hint">
            0.10–1.00 · Nucleus sampling cutoff for acoustic token selection.
          </div>
        </div>

        <div className="slider-field">
          <div className="label-row">
            <label htmlFor={`${idPrefix}-repetition-penalty`}>Repetition Penalty</label>
            <span className="mono badge badge-neutral">
              {value.repetition_penalty.toFixed(2)}
            </span>
          </div>
          <input
            id={`${idPrefix}-repetition-penalty`}
            type="range"
            min={1.0}
            max={12.0}
            step={0.25}
            value={value.repetition_penalty}
            onChange={(e) => updateField('repetition_penalty', Number(e.target.value))}
          />
          <div className="field-hint">
            1.0–12.0 · Prevents syllable looping or trailing vowel stalls.
          </div>
        </div>

        <div className="slider-field">
          <div className="label-row">
            <label htmlFor={`${idPrefix}-speed`}>Speaking Speed</label>
            <span className="mono badge badge-neutral">{value.speed.toFixed(2)}x</span>
          </div>
          <input
            id={`${idPrefix}-speed`}
            type="range"
            min={0.5}
            max={1.75}
            step={0.05}
            value={value.speed}
            onChange={(e) => updateField('speed', Number(e.target.value))}
          />
          <div className="field-hint">
            0.50–1.75x · Adjusts spoken delivery cadence without pitch-shifting timbre.
          </div>
        </div>

        <div className="slider-field">
          <div className="label-row">
            <label htmlFor={`${idPrefix}-gpt-cond-len`}>
              GPT Conditioning Window (s)
            </label>
            <span className="mono badge badge-neutral">
              {value.gpt_cond_len_secs}s
            </span>
          </div>
          <input
            id={`${idPrefix}-gpt-cond-len`}
            type="range"
            min={6}
            max={60}
            step={2}
            value={value.gpt_cond_len_secs}
            onChange={(e) =>
              updateField('gpt_cond_len_secs', Math.round(Number(e.target.value)))
            }
          />
          <div className="field-hint">
            6–60s · Total reference audio seconds conditioned into XTTS-v2 speaker latents.
          </div>
        </div>

        <div className="slider-field">
          <div className="label-row">
            <label htmlFor={`${idPrefix}-chunk-len`}>Chunk Window (s)</label>
            <span className="mono badge badge-neutral">
              {value.gpt_cond_chunk_len_secs}s
            </span>
          </div>
          <input
            id={`${idPrefix}-chunk-len`}
            type="range"
            min={2}
            max={12}
            step={1}
            value={value.gpt_cond_chunk_len_secs}
            onChange={(e) =>
              updateField('gpt_cond_chunk_len_secs', Math.round(Number(e.target.value)))
            }
          />
          <div className="field-hint">
            2–12s · Latent averaging window per conditioning slice (4s recommended).
          </div>
        </div>
      </div>

      <label className="checkbox-row" htmlFor={`${idPrefix}-sound-norm`}>
        <input
          id={`${idPrefix}-sound-norm`}
          type="checkbox"
          checked={value.sound_norm_refs}
          onChange={(e) => updateField('sound_norm_refs', e.target.checked)}
        />
        <span>
          <strong>Loudness-Normalize Reference Clips</strong> — Balances gain across
          multi-sample reference recordings before latent extraction.
        </span>
      </label>
    </div>
  );
};
