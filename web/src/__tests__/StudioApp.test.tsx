import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Provider } from 'react-redux';
import { encodeMonoWav24k } from '../audio/wavEncoder';
import {
  buildDirectRenderMultipartCurl,
  buildVoiceJobJsonCurl,
} from '../api/curlBuilder';
import { createAppStore } from '../store/store';
import {
  DEFAULT_SUPPORTED_LANGUAGES,
  DEFAULT_SYNTHESIS_TUNING,
  ENROLLMENT_READING_SCRIPTS,
  isValidProfileSlug,
} from '../types/api';
import { App } from '../App';

function jsonResponse(body: unknown, status = 200, contentType = 'application/json'): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': contentType },
  });
}

const MOCK_CONFIG = {
  service_name: 'discord-polyglot-voice',
  discord_owner_user_id: 987654321,
  discord_application_id: 123456789,
  default_target_language: 'zh',
  max_cer_bps: 1200,
  max_tts_attempts: 3,
  supported_engines: ['cosyvoice_3', 'xtts_v2'],
  supported_delivery_modes: ['client_return_only', 'profile_webhook', 'bot_voice_bubble'],
};

const MOCK_LANGUAGES = [
  {
    code: 'zh',
    english_name: 'Mandarin Chinese',
    native_name: '中文 (普通话)',
    whisper_code: 'zh',
    xtts_code: 'zh-cn',
    supported_engines: ['cosyvoice_3', 'xtts_v2'],
    recommended_engine: 'cosyvoice_3',
    phonetic_label: 'Pinyin (Tone-Marked)',
    sample_prompt: 'Let us meet at 7:15 tonight for dinner.',
  },
  {
    code: 'es',
    english_name: 'Spanish',
    native_name: 'Español',
    whisper_code: 'es',
    xtts_code: 'es',
    supported_engines: ['xtts_v2'],
    recommended_engine: 'xtts_v2',
    phonetic_label: 'Spanish Stress & Pronunciation Guide',
    sample_prompt: 'Hey team, let us jump into the Discord call at 3:30 PM!',
  },
  {
    code: 'ja',
    english_name: 'Japanese',
    native_name: '日本語',
    whisper_code: 'ja',
    xtts_code: 'ja',
    supported_engines: ['cosyvoice_3', 'xtts_v2'],
    recommended_engine: 'cosyvoice_3',
    phonetic_label: 'Romaji',
    sample_prompt: 'Thank you for waiting, joining in two minutes.',
  },
];

const MOCK_PROFILES = {
  items: [
    {
      id: '01933c89-0000-7000-8000-000000000001',
      slug: 'default_voice',
      display_name: 'Primary Studio Mic',
      avatar_url: null,
      reference_wav_path: '/data/profiles/default_voice.wav',
      reference_transcript_en: 'When sunlight strikes raindrops in the air, they act like a prism.',
      preferred_engine: 'cosyvoice_3',
      default_target_language: 'zh',
      owner_discord_user_id: 987654321,
      created_at: '2026-11-18T04:00:00Z',
    },
  ],
  total: 1,
  limit: 50,
  offset: 0,
};

describe('Polyglot Voice Clone Studio (Redux + Fetch Integration)', () => {
  const fetchMock = vi.fn<typeof fetch>();

  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal('fetch', fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function installBaseHandlers(
    customHandler?: (url: string, method: string, init?: RequestInit) => Response | Promise<Response> | null,
  ) {
    fetchMock.mockImplementation(async (input, init) => {
      const url =
        typeof input === 'string'
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url;
      const method = init?.method ?? 'GET';

      if (customHandler) {
        const custom = await customHandler(url, method, init);
        if (custom) {
          return custom;
        }
      }

      if (url.endsWith('/health/ready') && method === 'GET') {
        return jsonResponse({ status: 'ready' });
      }
      if (url.endsWith('/api/v1/config') && method === 'GET') {
        return jsonResponse(MOCK_CONFIG);
      }
      if (url.endsWith('/api/v1/languages') && method === 'GET') {
        return jsonResponse(MOCK_LANGUAGES);
      }
      if (
        url.includes('/api/v1/profiles') &&
        method === 'GET' &&
        !url.includes('/reference-audio')
      ) {
        return jsonResponse(MOCK_PROFILES);
      }
      if (
        url.includes('/api/v1/voice-jobs') &&
        method === 'GET' &&
        !url.includes('/audio')
      ) {
        return jsonResponse({ items: [], total: 0, limit: 20, offset: 0 });
      }
      if (url.includes('/api/v1/benchmarks') && method === 'GET') {
        return jsonResponse({
          items: [
            {
              id: 'bench-001',
              engine_tested: 'cosyvoice_3',
              clips_evaluated: 15,
              gate0_accuracy_bps: 10000,
              digit_hygiene_bps: 10000,
              mean_cer_bps: 420,
              mean_speaker_sim_bps: 8350,
              format_compliance_bps: 10000,
              p95_latency_ms: 3420,
              peak_vram_mib: 19840,
              passed_all_gates: true,
              created_at: '2026-11-18T05:00:00Z',
            },
          ],
          total: 1,
          limit: 10,
          offset: 0,
        });
      }

      return jsonResponse(
        {
          type: 'urn:discord-mandarin-voice:problem:not-found',
          title: 'Not Found',
          status: 404,
          detail: `Unhandled mock route: ${method} ${url}`,
        },
        404,
        'application/problem+json',
      );
    });
  }

  it('loads runtime config, languages, and profiles into Redux and updates recommended engine + curl snippet when switching Mandarin -> Spanish', async () => {
    installBaseHandlers();
    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().connection.healthStatus).toBe('ready');
      expect(store.getState().profiles.items).toHaveLength(1);
      expect(store.getState().languages.items).toHaveLength(3);
    });

    const configCall = fetchMock.mock.calls.find(([u]) =>
      String(u).endsWith('/api/v1/config'),
    );
    expect(configCall).toBeDefined();
    const headers = new Headers(configCall?.[1]?.headers);
    expect(headers.get('Authorization')).toBe(
      'Bearer dev-owner-bearer-token-change-me',
    );

    expect(screen.getByTestId('recommended-engine-badge')).toHaveTextContent(
      /cosyvoice_3/i,
    );
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      '"target_language": "zh"',
    );

    const langSelect = screen.getByLabelText(/Target Language/i);
    await user.selectOptions(langSelect, 'es');

    expect(store.getState().studio.targetLanguage).toBe('es');
    expect(screen.getByTestId('recommended-engine-badge')).toHaveTextContent(
      /xtts_v2/i,
    );
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      '"target_language": "es"',
    );
  });

  it('enrolls a new voice profile with base64 WAV audio and 1-click guided script', async () => {
    let capturedEnrollBody: Record<string, unknown> | null = null;

    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/profiles') && method === 'POST') {
        capturedEnrollBody = JSON.parse(String(init?.body ?? '{}')) as Record<
          string,
          unknown
        >;
        return jsonResponse(
          {
            id: '01933c89-0000-7000-8000-000000000002',
            slug: capturedEnrollBody.slug,
            display_name: capturedEnrollBody.display_name,
            avatar_url: capturedEnrollBody.avatar_url,
            reference_wav_path: `/data/profiles/${String(capturedEnrollBody.slug)}.wav`,
            reference_transcript_en: capturedEnrollBody.reference_transcript_en,
            preferred_engine: capturedEnrollBody.preferred_engine,
            default_target_language: capturedEnrollBody.default_target_language,
            owner_discord_user_id: 987654321,
            created_at: '2026-11-18T05:30:00Z',
          },
          201,
        );
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().profiles.items).toHaveLength(1);
    });

    // Switch to Enrollment Studio tab
    await user.click(screen.getByRole('button', { name: /2\. Voice Recording & Enrollment Studio/i }));

    // Click the second guided script button to 1-click populate transcript
    const scriptButtons = screen.getAllByRole('button', {
      name: /Use This Script & Populate Transcript/i,
    });
    await user.click(scriptButtons[1]!);

    const transcriptBox = screen.getByLabelText(/Exact Spoken English Transcript/i);
    expect(transcriptBox).toHaveValue(
      'Good morning! I am grabbing a quick coffee right now, and then I will jump into the voice channel around seven thirty. Let me know if the audio sounds crisp and warm on your end before we invite the rest of the team.',
    );

    // Upload a valid 1-second 24 kHz WAV file
    const sine = new Float32Array(24_000);
    for (let i = 0; i < sine.length; i += 1) {
      sine[i] = 0.4 * Math.sin((2 * Math.PI * 300 * i) / 24_000);
    }
    const { wavBytes } = encodeMonoWav24k(sine, 24_000);
    const wavArrayBuf = new ArrayBuffer(wavBytes.byteLength);
    new Uint8Array(wavArrayBuf).set(wavBytes);
    const wavFile = new File([wavArrayBuf], 'sample-ref.wav', { type: 'audio/wav' });

    const fileInput = screen.getByLabelText(/Upload Reference Audio File/i);
    await user.upload(fileInput, wavFile);

    await waitFor(() => {
      expect(screen.getByTestId('captured-audio-card')).toBeInTheDocument();
    });

    const slugInput = screen.getByLabelText(/Profile Slug/i);
    await user.clear(slugInput);
    await user.type(slugInput, 'polyglot_voice');

    await user.click(screen.getByRole('button', { name: /Enroll Voice Profile/i }));

    await waitFor(() => {
      expect(store.getState().profiles.items).toHaveLength(2);
      expect(store.getState().studio.selectedProfileSlug).toBe('polyglot_voice');
    });

    expect(capturedEnrollBody).not.toBeNull();
    expect(capturedEnrollBody?.['slug']).toBe('polyglot_voice');
    expect(typeof capturedEnrollBody?.['reference_audio_bytes']).toBe('string');
    expect(String(capturedEnrollBody?.['reference_audio_bytes']).startsWith('UklGR')).toBe(
      true,
    );
  });

  it('submits a multilingual Spanish voice job, renders full CER & phonetic receipt, and surfaces RFC 7807 detail on error', async () => {
    let shouldFailNextJob = false;
    let submittedJobPayload: Record<string, unknown> | null = null;

    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/voice-jobs') && method === 'POST') {
        submittedJobPayload = JSON.parse(String(init?.body ?? '{}')) as Record<
          string,
          unknown
        >;
        if (shouldFailNextJob) {
          return jsonResponse(
            {
              type: 'urn:discord-mandarin-voice:problem:validation-failed',
              title: 'Validation Failed',
              status: 422,
              detail:
                'CER gate failed after 3 seed attempts: best CER 1480 bps exceeds max 1200 bps',
            },
            422,
            'application/problem+json',
          );
        }
        return jsonResponse(
          {
            job_id: '01933c99-1111-7000-8000-000000000099',
            profile_id: '01933c89-0000-7000-8000-000000000001',
            profile_slug: 'default_voice',
            target_language: 'es',
            delivery_mode: 'client_return_only',
            english_transcript: 'See you at 3:30 PM on Discord!',
            spoken_text: '¡Nos vemos a las tres y media de la tarde en Discord!',
            phonetic_guide: 'nos BE-mos a las tres i ME-dya de la TAR-de en DIS-kord',
            english_back_gloss: 'See you at three thirty in the afternoon on Discord!',
            back_transcribed_text: 'Nos vemos a las tres y media de la tarde en Discord',
            engine_used: 'xtts_v2',
            cer_bps: 180,
            attempts_used: 1,
            duration_ms: 3200,
            total_latency_ms: 2150,
            output_ogg_path: '/data/jobs/01933c99.ogg',
            discord_message_id: null,
            created_at: '2026-11-18T05:40:00Z',
          },
          201,
        );
      }

      if (url.includes('/api/v1/voice-jobs/01933c99-1111-7000-8000-000000000099/audio')) {
        return new Response(new Uint8Array([0x4f, 0x67, 0x67, 0x53]), {
          status: 200,
          headers: { 'content-type': 'audio/ogg' },
        });
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().studio.selectedProfileSlug).toBe('default_voice');
    });

    await user.selectOptions(screen.getByLabelText(/Target Language/i), 'es');

    const promptInput = screen.getByLabelText(/English Source Text/i);
    await user.clear(promptInput);
    await user.type(promptInput, 'See you at 3:30 PM on Discord!');

    await user.click(
      screen.getByRole('button', { name: /Clone Voice & Translate to Spanish/i }),
    );

    await waitFor(() => {
      expect(screen.getByTestId('voice-job-receipt')).toBeInTheDocument();
    });

    expect(submittedJobPayload).toMatchObject({
      profile_slug: 'default_voice',
      target_language: 'es',
      engine_override: null,
      source_text: 'See you at 3:30 PM on Discord!',
      delivery: { mode: 'client_return_only' },
    });

    expect(screen.getByTestId('receipt-cer-badge')).toHaveTextContent(
      'CER: 180 bps (1.80%)',
    );
    expect(screen.getByTestId('voice-job-receipt')).toHaveTextContent(
      '¡Nos vemos a las tres y media de la tarde en Discord!',
    );
    expect(screen.getByTestId('voice-job-receipt')).toHaveTextContent(
      'nos BE-mos a las tres i ME-dya de la TAR-de en DIS-kord',
    );

    // Now verify RFC 7807 Problem Details `detail` surfaces cleanly in Redux + UI
    shouldFailNextJob = true;
    await user.click(
      screen.getByRole('button', { name: /Clone Voice & Translate to Spanish/i }),
    );

    await waitFor(() => {
      expect(screen.getByTestId('studio-error-alert')).toHaveTextContent(
        'CER gate failed after 3 seed attempts: best CER 1480 bps exceeds max 1200 bps',
      );
    });
    expect(store.getState().studio.error).toBe(
      'CER gate failed after 3 seed attempts: best CER 1480 bps exceeds max 1200 bps',
    );
  });

  it('supports editing & deleting profiles and inspecting runtime config & benchmarks in History tab', async () => {
    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/profiles/default_voice') && method === 'PATCH') {
        const patch = JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>;
        return jsonResponse({
          ...MOCK_PROFILES.items[0],
          ...patch,
        });
      }
      if (url.endsWith('/api/v1/profiles/default_voice') && method === 'DELETE') {
        return new Response(null, { status: 204 });
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().profiles.items).toHaveLength(1);
    });

    // Open Profile Management tab
    await user.click(screen.getByRole('button', { name: /3\. Profile Management/i }));
    await user.click(screen.getByRole('button', { name: /^Edit$/i }));

    const editName = screen.getByLabelText(/Display Name/i);
    await user.clear(editName);
    await user.type(editName, 'Updated Studio Mic');
    await user.click(screen.getByRole('button', { name: /Save Changes/i }));

    await waitFor(() => {
      expect(store.getState().profiles.items[0]?.display_name).toBe('Updated Studio Mic');
    });

    await user.click(screen.getByRole('button', { name: /^Delete$/i }));
    await waitFor(() => {
      expect(store.getState().profiles.items).toHaveLength(0);
    });

    // Open History, Config & Benchmarks tab
    await user.click(
      screen.getByRole('button', { name: /4\. History, Config & Benchmarks/i }),
    );

    await waitFor(() => {
      expect(screen.getByTestId('runtime-config-inspector')).toHaveTextContent(
        'discord-polyglot-voice',
      );
      expect(store.getState().history.benchmarks).toHaveLength(1);
    });
    expect(screen.getByText('19840 MiB')).toBeInTheDocument();
  });

  it('defaults English recommended_engine to xtts_v2 and includes x-discord-user-id in /render cURL when configured', () => {
    const englishEntry = DEFAULT_SUPPORTED_LANGUAGES.find((l) => l.code === 'en');
    expect(englishEntry).toBeDefined();
    expect(englishEntry?.recommended_engine).toBe('xtts_v2');

    const curlWithDiscordId = buildDirectRenderMultipartCurl({
      apiBaseUrl: 'http://localhost:8080',
      discordUserId: '1234567890123456789',
      profileSlug: 'default_voice',
      targetLanguage: 'en',
      engineSelection: 'auto',
      maxCerBpsOverride: '',
      inputMode: 'text',
      sourceText: 'Hello there!',
      outputFormat: 'ogg',
      delivery: { mode: 'client_return_only' },
    });
    expect(curlWithDiscordId).toContain('-H "x-discord-user-id: 1234567890123456789"');

    const curlWithoutDiscordId = buildDirectRenderMultipartCurl({
      apiBaseUrl: 'http://localhost:8080',
      discordUserId: '   ',
      profileSlug: 'default_voice',
      targetLanguage: 'en',
      engineSelection: 'auto',
      maxCerBpsOverride: '',
      inputMode: 'text',
      sourceText: 'Hello there!',
      outputFormat: 'ogg',
      delivery: { mode: 'client_return_only' },
    });
    expect(curlWithoutDiscordId).not.toContain('x-discord-user-id');
  });

  it('preserves 19-digit 64-bit Discord snowflakes as exact strings in BotVoiceBubble payloads, cURL snippets, and API responses without rounding', async () => {
    let rawVoiceJobRequestBody = '';
    let voiceJobPostCount = 0;

    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/config') && method === 'GET') {
        return jsonResponse({
          ...MOCK_CONFIG,
          discord_owner_user_id: 1234567890123456800,
          discord_owner_user_id_str: '1234567890123456781',
          discord_application_id: 1234567890123456800,
          discord_application_id_str: '1234567890123456782',
        });
      }
      if (
        url.includes('/api/v1/profiles') &&
        method === 'GET' &&
        !url.includes('/reference-audio')
      ) {
        return jsonResponse({
          ...MOCK_PROFILES,
          items: [
            {
              ...MOCK_PROFILES.items[0],
              owner_discord_user_id: '1234567890123456781',
            },
          ],
        });
      }
      if (url.endsWith('/api/v1/voice-jobs') && method === 'POST') {
        voiceJobPostCount += 1;
        rawVoiceJobRequestBody = String(init?.body ?? '');
        return jsonResponse(
          {
            job_id: '01933c99-2222-7000-8000-000000000088',
            profile_id: '01933c89-0000-7000-8000-000000000001',
            profile_slug: 'default_voice',
            target_language: 'zh',
            delivery_mode: 'bot_voice_bubble',
            english_transcript: 'Testing 64-bit snowflakes',
            spoken_text: '测试六十四位雪花标识',
            phonetic_guide: 'cè shì liù shí sì wèi xuě huā biāo shí',
            english_back_gloss: 'Testing sixty-four bit snowflake identifiers',
            back_transcribed_text: '测试六十四位雪花标识',
            engine_used: 'cosyvoice_3',
            cer_bps: 0,
            attempts_used: 1,
            duration_ms: 2100,
            total_latency_ms: 1400,
            output_ogg_path: '/data/jobs/01933c99-2222.ogg',
            discord_message_id: 1234567890123456800,
            discord_message_id_str: '1234567890123456787',
            created_at: '2026-11-18T06:00:00Z',
          },
          201,
        );
      }
      if (url.includes('/api/v1/voice-jobs/01933c99-2222-7000-8000-000000000088/audio')) {
        return new Response(new Uint8Array([0x4f, 0x67, 0x67, 0x53]), {
          status: 200,
          headers: { 'content-type': 'audio/ogg' },
        });
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().studio.selectedProfileSlug).toBe('default_voice');
      expect(store.getState().connection.runtimeConfig?.discord_owner_user_id).toBe(
        '1234567890123456781',
      );
      expect(store.getState().connection.runtimeConfig?.discord_application_id).toBe(
        '1234567890123456782',
      );
      expect(store.getState().profiles.items[0]?.owner_discord_user_id).toBe(
        '1234567890123456781',
      );
    });

    await user.selectOptions(
      screen.getByLabelText(/Delivery Mode/i),
      'bot_voice_bubble',
    );

    const channelInput = screen.getByLabelText(/Target Discord Channel ID/i);
    const replyInput = screen.getByLabelText(/Reply To Message ID/i);

    // Overflow u64::MAX + 1 must be rejected client-side without POSTing
    await user.type(channelInput, '18446744073709551616');
    await user.click(
      screen.getByRole('button', { name: /Clone Voice & Translate to Mandarin/i }),
    );
    await waitFor(() => {
      expect(screen.getByTestId('studio-error-alert')).toBeInTheDocument();
    });
    expect(voiceJobPostCount).toBe(0);

    // Valid 19-digit snowflakes (> 2^53 - 1) must serialize as exact digit strings
    await user.clear(channelInput);
    await user.type(channelInput, '1234567890123456789');
    await user.type(replyInput, '1234567890123456788');

    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      '"channel_id": "1234567890123456789"',
    );
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      '"reply_to_message_id": "1234567890123456788"',
    );

    await user.click(
      screen.getByRole('button', { name: /Clone Voice & Translate to Mandarin/i }),
    );

    await waitFor(() => {
      expect(voiceJobPostCount).toBe(1);
      expect(screen.getByTestId('voice-job-receipt')).toHaveTextContent(
        '1234567890123456787',
      );
    });

    const parsedSent = JSON.parse(rawVoiceJobRequestBody) as {
      delivery: {
        mode: string;
        channel_id: unknown;
        reply_to_message_id: unknown;
      };
    };
    expect(parsedSent.delivery).toEqual({
      mode: 'bot_voice_bubble',
      channel_id: '1234567890123456789',
      reply_to_message_id: '1234567890123456788',
    });
    expect(store.getState().studio.lastReceipt?.discord_message_id).toBe(
      '1234567890123456787',
    );
  });

  it('enforces backend ^[a-z][a-z0-9_-]{1,63}$ slug constraints (minLength 2, maxLength 64, leading lowercase letter) in EnrollmentTab', async () => {
    let enrollPostCount = 0;
    installBaseHandlers((url, method) => {
      if (url.endsWith('/api/v1/profiles') && method === 'POST') {
        enrollPostCount += 1;
      }
      return null;
    });

    expect(isValidProfileSlug('ab')).toBe(true);
    expect(isValidProfileSlug('studio_voice-01')).toBe(true);
    expect(isValidProfileSlug('a' + 'b'.repeat(63))).toBe(true);
    expect(isValidProfileSlug('a')).toBe(false);
    expect(isValidProfileSlug('1voice')).toBe(false);
    expect(isValidProfileSlug('_voice')).toBe(false);
    expect(isValidProfileSlug('Studio_Voice')).toBe(false);
    expect(isValidProfileSlug('a' + 'b'.repeat(64))).toBe(false);

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await user.click(
      screen.getByRole('button', { name: /2\. Voice Recording & Enrollment Studio/i }),
    );

    const slugInput = screen.getByLabelText(/Profile Slug/i);
    expect(slugInput).toHaveAttribute('pattern', '^[a-z][a-z0-9_-]{1,63}$');
    expect(slugInput).toHaveAttribute('minlength', '2');
    expect(slugInput).toHaveAttribute('maxlength', '64');

    // Upload a valid 1-second 24 kHz WAV file so Gate 0 passes and slug validation is exercised
    const sine = new Float32Array(24_000);
    for (let i = 0; i < sine.length; i += 1) {
      sine[i] = 0.4 * Math.sin((2 * Math.PI * 300 * i) / 24_000);
    }
    const { wavBytes } = encodeMonoWav24k(sine, 24_000);
    const wavArrayBuf = new ArrayBuffer(wavBytes.byteLength);
    new Uint8Array(wavArrayBuf).set(wavBytes);
    const wavFile = new File([wavArrayBuf], 'sample-ref.wav', { type: 'audio/wav' });
    await user.upload(screen.getByLabelText(/Upload Reference Audio File/i), wavFile);

    await waitFor(() => {
      expect(screen.getByTestId('captured-audio-card')).toBeInTheDocument();
    });

    // Leading digit ("1bad_slug") must fail client-side slug validation with an alert and zero POSTs
    await user.clear(slugInput);
    await user.type(slugInput, '1bad_slug');
    await user.click(screen.getByRole('button', { name: /Enroll Voice Profile/i }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(/2–64/);
    });
    expect(enrollPostCount).toBe(0);
  });

  it('renders dynamic 1-on-1 Discord OAuth quick-launch links from discord_application_id_str and shows setup-discord fallback when unconfigured', async () => {
    let currentAppIdStr = '1234567890123456782';
    let currentAppIdNum = 1234567890123456800;

    installBaseHandlers((url, method) => {
      if (url.endsWith('/api/v1/config') && method === 'GET') {
        return jsonResponse({
          ...MOCK_CONFIG,
          discord_application_id: currentAppIdNum,
          discord_application_id_str: currentAppIdStr,
        });
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    const { unmount } = render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().connection.runtimeConfig?.discord_application_id_str).toBe(
        '1234567890123456782',
      );
    });

    await user.click(
      screen.getByRole('button', { name: /5\. How Recording & Delivery Works/i }),
    );

    const userInstallLink = screen.getByRole('link', {
      name: /1\. Install to Personal Account/i,
    });
    expect(userInstallLink).toHaveAttribute(
      'href',
      'https://discord.com/oauth2/authorize?client_id=1234567890123456782&integration_type=1&scope=applications.commands',
    );

    const botDmLink = screen.getByRole('link', {
      name: /2\. Authorize Bot & Open 1-on-1 Bot DM/i,
    });
    expect(botDmLink).toHaveAttribute(
      'href',
      'https://discord.com/oauth2/authorize?client_id=1234567890123456782&integration_type=0&scope=bot+applications.commands&permissions=274877910016',
    );

    const quickLaunchCard = screen.getByTestId('discord-quick-launch-card');
    expect(quickLaunchCard).toHaveTextContent('/voice_clone');
    expect(quickLaunchCard).toHaveTextContent('/voice_settings');
    expect(quickLaunchCard).toHaveTextContent('/voice_profiles');
    expect(quickLaunchCard).toHaveTextContent('es:');

    unmount();

    // Verify placeholder / 0 fallback prompts running `just setup-discord`
    currentAppIdStr = '0';
    currentAppIdNum = 0;
    const unconfiguredStore = createAppStore();

    render(
      <Provider store={unconfiguredStore}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(unconfiguredStore.getState().connection.runtimeConfig?.discord_application_id).toBe(
        '0',
      );
    });

    await user.click(
      screen.getByRole('button', { name: /5\. How Recording & Delivery Works/i }),
    );

    expect(
      screen.getByText(/Run just setup-discord to populate DISCORD_APPLICATION_ID/i),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: /1\. Install to Personal Account/i }),
    ).not.toBeInTheDocument();
  });

  it('defaults cURL snippets to http://localhost:8080 and adds -k (curl -ksS) for https://192.0.2.10:3443 self-signed SAN TLS', () => {
    const baseInput = {
      apiBaseUrl: '',
      discordUserId: '',
      profileSlug: 'default_voice',
      targetLanguage: 'es',
      engineSelection: 'auto' as const,
      maxCerBpsOverride: '',
      inputMode: 'text' as const,
      sourceText: 'Joining from LAN!',
      outputFormat: 'ogg' as const,
      delivery: { mode: 'client_return_only' as const },
    };

    // Empty apiBaseUrl on localhost falls back to local API http://localhost:8080 with standard -sS
    const defaultJsonCurl = buildVoiceJobJsonCurl(baseInput);
    expect(defaultJsonCurl).toContain(
      'curl -sS -X POST "http://localhost:8080/api/v1/voice-jobs"',
    );

    const defaultRenderCurl = buildDirectRenderMultipartCurl(baseInput);
    expect(defaultRenderCurl).toContain(
      'curl -sS -X POST "http://localhost:8080/api/v1/voice-jobs/render?',
    );

    // Self-signed SAN HTTPS on :3443 includes -k (-ksS) so copy-pasted commands succeed immediately
    const tlsJsonCurl = buildVoiceJobJsonCurl({
      ...baseInput,
      apiBaseUrl: 'https://192.0.2.10:3443',
    });
    expect(tlsJsonCurl).toContain(
      'curl -ksS -X POST "https://192.0.2.10:3443/api/v1/voice-jobs"',
    );

    const tlsRenderCurl = buildDirectRenderMultipartCurl({
      ...baseInput,
      apiBaseUrl: 'https://192.0.2.10:3443',
    });
    expect(tlsRenderCurl).toContain(
      'curl -ksS -X POST "https://192.0.2.10:3443/api/v1/voice-jobs/render?',
    );
  });

  it('shows a 1-click https://192.0.2.10:3443 mic callout on plain LAN HTTP (:3000) and hides it on SAN HTTPS (:3443)', async () => {
    installBaseHandlers();
    const originalLocation = window.location;
    const originalMediaDevices = navigator.mediaDevices;

    try {
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: {
          ...originalLocation,
          protocol: 'http:',
          hostname: '192.0.2.10',
          port: '3000',
          host: '192.0.2.10:3000',
          origin: 'http://192.0.2.10:3000',
        },
      });
      Object.defineProperty(navigator, 'mediaDevices', {
        configurable: true,
        value: undefined,
      });

      const user = userEvent.setup();
      const httpStore = createAppStore();
      const { unmount } = render(
        <Provider store={httpStore}>
          <App />
        </Provider>,
      );

      await user.click(
        screen.getByRole('button', {
          name: /2\. Voice Recording & Enrollment Studio/i,
        }),
      );

      const callout = screen.getByTestId('lan-https-mic-callout');
      expect(callout).toHaveTextContent(
        /Live browser microphone capture over LAN requires HTTPS/i,
      );
      const httpsLink = screen.getByRole('link', {
        name: /https:\/\/192\.0\.2\.10:3443/i,
      });
      expect(httpsLink).toHaveAttribute('href', 'https://192.0.2.10:3443');

      await user.click(
        screen.getByRole('button', { name: /Record Microphone \(24 kHz WAV\)/i }),
      );
      expect(screen.getByRole('alert')).toHaveTextContent(
        'https://192.0.2.10:3443',
      );

      unmount();

      // Now simulate opening over SAN HTTPS at https://192.0.2.10:3443
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: {
          ...originalLocation,
          protocol: 'https:',
          hostname: '192.0.2.10',
          port: '3443',
          host: '192.0.2.10:3443',
          origin: 'https://192.0.2.10:3443',
        },
      });

      const httpsStore = createAppStore();
      render(
        <Provider store={httpsStore}>
          <App />
        </Provider>,
      );

      expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
        'curl -ksS -X POST "https://192.0.2.10:3443/api/v1/voice-jobs"',
      );

      await user.click(
        screen.getByRole('button', {
          name: /2\. Voice Recording & Enrollment Studio/i,
        }),
      );
      expect(screen.queryByTestId('lan-https-mic-callout')).not.toBeInTheDocument();

      await user.click(
        screen.getByRole('button', { name: /5\. How Recording & Delivery Works/i }),
      );
      expect(
        screen.getAllByText(/https:\/\/192\.0\.2\.10:3443/).length,
      ).toBeGreaterThan(0);
      expect(
        screen.getAllByText(/http:\/\/192\.0\.2\.10:8080/).length,
      ).toBeGreaterThan(0);
    } finally {
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: originalLocation,
      });
      Object.defineProperty(navigator, 'mediaDevices', {
        configurable: true,
        value: originalMediaDevices,
      });
    }
  });

  it('adds a new reference sample to an existing profile (POST /api/v1/profiles/default_voice/samples) and updates multi-sample count & tier badge', async () => {
    let capturedSamplePayload: Record<string, unknown> | null = null;

    expect(ENROLLMENT_READING_SCRIPTS).toHaveLength(6);

    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/profiles/default_voice/samples') && method === 'POST') {
        capturedSamplePayload = JSON.parse(String(init?.body ?? '{}')) as Record<
          string,
          unknown
        >;
        return jsonResponse(
          {
            ...MOCK_PROFILES.items[0],
            sample_count: 3,
            total_sample_duration_ms: 46500,
            samples: [
              {
                id: 'samp-001',
                profile_id: '01933c89-0000-7000-8000-000000000001',
                wav_path: '/data/profiles/default_voice/samp-001.wav',
                transcript_en:
                  'When sunlight strikes raindrops in the air, they act like a prism.',
                style_label: 'conversational',
                duration_ms: 15500,
                is_primary: true,
                created_at: '2026-11-18T04:00:00Z',
              },
              {
                id: 'samp-002',
                profile_id: '01933c89-0000-7000-8000-000000000001',
                wav_path: '/data/profiles/default_voice/samp-002.wav',
                transcript_en: 'Good morning! Grabbing a quick coffee right now.',
                style_label: 'calm',
                duration_ms: 15000,
                is_primary: false,
                created_at: '2026-11-18T04:05:00Z',
              },
              {
                id: 'samp-003',
                profile_id: '01933c89-0000-7000-8000-000000000001',
                wav_path: '/data/profiles/default_voice/samp-003.wav',
                transcript_en: String(capturedSamplePayload.transcript_en),
                style_label: String(capturedSamplePayload.style_label),
                duration_ms: 16000,
                is_primary: false,
                created_at: '2026-11-18T04:10:00Z',
              },
            ],
          },
          201,
        );
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().profiles.items).toHaveLength(1);
    });

    // Legacy mock profile without tuning/sample_count fields normalizes cleanly with defaults
    expect(store.getState().profiles.items[0]?.sample_count).toBe(1);
    expect(store.getState().profiles.items[0]?.tuning).toEqual(DEFAULT_SYNTHESIS_TUNING);

    // Open Tab 3 -> expand Manage Clips, Tuning & RTX 3090 Fine-Tune
    await user.click(screen.getByRole('button', { name: /3\. Profile Management/i }));
    await user.click(
      screen.getByRole('button', {
        name: /Manage Clips, Tuning & RTX 3090 Fine-Tune/i,
      }),
    );

    expect(
      screen.getByTestId('profile-likeness-tier-default_voice'),
    ).toHaveTextContent('1 clip · Basic');

    // Select expressive style and upload a 1-second 24 kHz WAV sample
    await user.selectOptions(
      screen.getByLabelText(/Delivery Style Tag/i),
      'expressive',
    );

    const sine = new Float32Array(24_000);
    for (let i = 0; i < sine.length; i += 1) {
      sine[i] = 0.35 * Math.sin((2 * Math.PI * 280 * i) / 24_000);
    }
    const { wavBytes } = encodeMonoWav24k(sine, 24_000);
    const wavArrayBuf = new ArrayBuffer(wavBytes.byteLength);
    new Uint8Array(wavArrayBuf).set(wavBytes);
    const wavFile = new File([wavArrayBuf], 'sample-extra-clip.wav', {
      type: 'audio/wav',
    });

    await user.upload(screen.getByLabelText(/Upload Reference Audio File/i), wavFile);

    await waitFor(() => {
      expect(screen.getByTestId('captured-audio-card')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: /\+ Add Sample to Profile/i }));

    await waitFor(() => {
      expect(store.getState().profiles.items[0]?.sample_count).toBe(3);
    });

    expect(capturedSamplePayload).not.toBeNull();
    expect(capturedSamplePayload?.['style_label']).toBe('expressive');
    expect(String(capturedSamplePayload?.['audio_bytes']).startsWith('UklGR')).toBe(
      true,
    );

    expect(
      screen.getByTestId('profile-sample-summary-default_voice'),
    ).toHaveTextContent('3 clips · 46.5s total');
    expect(
      screen.getByTestId('profile-likeness-tier-default_voice'),
    ).toHaveTextContent('3+ clips · Multi-Sample Averaged');
    expect(screen.getByTestId('profile-sample-row-samp-003')).toBeInTheDocument();
  });

  it('sends tuning_override in POST /api/v1/voice-jobs and live cURL snippets when Playground custom synthesis tuning is enabled', async () => {
    let submittedJobPayload: Record<string, unknown> | null = null;

    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/voice-jobs') && method === 'POST') {
        submittedJobPayload = JSON.parse(String(init?.body ?? '{}')) as Record<
          string,
          unknown
        >;
        return jsonResponse(
          {
            job_id: '01933c99-4444-7000-8000-000000000044',
            profile_id: '01933c89-0000-7000-8000-000000000001',
            profile_slug: 'default_voice',
            target_language: 'zh',
            delivery_mode: 'client_return_only',
            english_transcript: 'Testing custom voice likeness tuning',
            spoken_text: '测试自定义声音相似度调优',
            phonetic_guide: 'cè shì zì dìng yì shēng yīn xiāng sì dù tiáo yōu',
            english_back_gloss: 'Testing custom voice similarity tuning',
            back_transcribed_text: '测试自定义声音相似度调优',
            engine_used: 'xtts_v2',
            cer_bps: 120,
            attempts_used: 1,
            duration_ms: 2800,
            total_latency_ms: 1900,
            output_ogg_path: '/data/jobs/01933c99-4444.ogg',
            discord_message_id: null,
            created_at: '2026-11-18T07:00:00Z',
          },
          201,
        );
      }
      if (url.includes('/api/v1/voice-jobs/01933c99-4444-7000-8000-000000000044/audio')) {
        return new Response(new Uint8Array([0x4f, 0x67, 0x67, 0x53]), {
          status: 200,
          headers: { 'content-type': 'audio/ogg' },
        });
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await waitFor(() => {
      expect(store.getState().studio.selectedProfileSlug).toBe('default_voice');
    });

    // By default, tuning_override is omitted from live cURL
    expect(screen.getByTestId('live-curl-snippet')).not.toHaveTextContent(
      'tuning_override',
    );

    // Toggle Override Voice Likeness & Synthesis Tuning on
    await user.click(
      screen.getByLabelText(
        /Override Voice Likeness & Synthesis Tuning for This Clip/i,
      ),
    );

    fireEvent.change(screen.getByLabelText(/^Temperature$/i), {
      target: { value: '0.52' },
    });
    fireEvent.change(screen.getByLabelText(/^Repetition Penalty$/i), {
      target: { value: '6.25' },
    });
    fireEvent.change(screen.getByLabelText(/GPT Conditioning Window \(s\)/i), {
      target: { value: '42' },
    });

    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      '"temperature": 0.52',
    );
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      '"repetition_penalty": 6.25',
    );
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      '"gpt_cond_len_secs": 42',
    );

    // Verify Binary Stream (/render) cURL also serializes query params
    await user.click(screen.getByRole('button', { name: /Binary Stream \(\/render\)/i }));
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      'temperature=0.52',
    );
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      'repetition_penalty=6.25',
    );
    expect(screen.getByTestId('live-curl-snippet')).toHaveTextContent(
      'gpt_cond_len_secs=42',
    );

    // Submit voice job and verify tuning_override in POST /api/v1/voice-jobs
    await user.click(
      screen.getByRole('button', { name: /Clone Voice & Translate to Mandarin/i }),
    );

    await waitFor(() => {
      expect(screen.getByTestId('voice-job-receipt')).toBeInTheDocument();
    });

    expect(submittedJobPayload?.['tuning_override']).toEqual({
      temperature: 0.52,
      top_p: 0.8,
      top_k: 50,
      repetition_penalty: 6.25,
      length_penalty: 1.0,
      speed: 1.0,
      gpt_cond_len_secs: 42,
      gpt_cond_chunk_len_secs: 4,
      max_ref_len_secs: 60,
      sound_norm_refs: true,
    });
  });

  it('triggers RTX 3090 fine-tuning (POST /api/v1/profiles/default_voice/finetune), polls progress to completion, and supports reverting to zero-shot base model', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let capturedFinetunePost: Record<string, unknown> | null = null;
    let capturedRevertPatch: Record<string, unknown> | null = null;
    let pollCount = 0;
    let finetuneCompletedOnServer = false;

    try {
      installBaseHandlers((url, method, init) => {
        if (url.endsWith('/api/v1/profiles/default_voice/finetune') && method === 'POST') {
          capturedFinetunePost = JSON.parse(String(init?.body ?? '{}')) as Record<
            string,
            unknown
          >;
          return jsonResponse(
            {
              id: 'ft-job-001',
              profile_id: '01933c89-0000-7000-8000-000000000001',
              profile_slug: 'default_voice',
              status: 'running',
              epochs: Number(capturedFinetunePost.epochs ?? 8),
              learning_rate: Number(capturedFinetunePost.learning_rate ?? 0.000005),
              sample_count: 4,
              current_epoch: 3,
              progress_bps: 3750,
              last_loss: 0.8421,
              checkpoint_dir: null,
              error_message: null,
              created_at: '2026-11-18T08:00:00Z',
              updated_at: '2026-11-18T08:01:00Z',
            },
            202,
          );
        }

        if (url.endsWith('/api/v1/profiles/default_voice/finetune') && method === 'GET') {
          pollCount += 1;
          finetuneCompletedOnServer = true;
          return jsonResponse({
            id: 'ft-job-001',
            profile_id: '01933c89-0000-7000-8000-000000000001',
            profile_slug: 'default_voice',
            status: 'succeeded',
            epochs: 8,
            learning_rate: 0.000005,
            sample_count: 4,
            current_epoch: 8,
            progress_bps: 10000,
            last_loss: 0.3105,
            checkpoint_dir: '/data/checkpoints/default_voice_xtts_gpt',
            error_message: null,
            created_at: '2026-11-18T08:00:00Z',
            updated_at: '2026-11-18T08:03:00Z',
          });
        }

        if (url.endsWith('/api/v1/profiles/default_voice') && method === 'PATCH') {
          capturedRevertPatch = JSON.parse(String(init?.body ?? '{}')) as Record<
            string,
            unknown
          >;
          finetuneCompletedOnServer = false;
          return jsonResponse({
            ...MOCK_PROFILES.items[0],
            sample_count: 4,
            total_sample_duration_ms: 62000,
            finetune_status: 'none',
            finetuned_checkpoint_dir: null,
          });
        }

        if (
          url.includes('/api/v1/profiles') &&
          method === 'GET' &&
          !url.includes('/reference-audio') &&
          !url.includes('/finetune')
        ) {
          return jsonResponse({
            ...MOCK_PROFILES,
            items: [
              {
                ...MOCK_PROFILES.items[0],
                sample_count: 4,
                total_sample_duration_ms: 62000,
                finetune_status: finetuneCompletedOnServer ? 'succeeded' : 'none',
                finetuned_checkpoint_dir: finetuneCompletedOnServer
                  ? '/data/checkpoints/default_voice_xtts_gpt'
                  : null,
              },
            ],
          });
        }

        return null;
      });

      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      const store = createAppStore();

      render(
        <Provider store={store}>
          <App />
        </Provider>,
      );

      await waitFor(() => {
        expect(store.getState().profiles.items).toHaveLength(1);
      });

      await user.click(screen.getByRole('button', { name: /3\. Profile Management/i }));
      await user.click(
        screen.getByRole('button', {
          name: /Manage Clips, Tuning & RTX 3090 Fine-Tune/i,
        }),
      );

      const epochsInput = screen.getByLabelText(/Epochs \(2–20/i);
      await user.clear(epochsInput);
      await user.type(epochsInput, '8');

      await user.click(
        screen.getByRole('button', { name: /Start RTX 3090 Fine-Tune/i }),
      );

      await waitFor(() => {
        expect(
          screen.getByTestId('finetune-progress-box-default_voice'),
        ).toHaveTextContent('Epoch 3 / 8 · Progress 38% · Loss 0.8421');
      });

      expect(capturedFinetunePost).toEqual({
        epochs: 8,
        learning_rate: 0.000005,
      });

      // Advance 2.5s auto-poll interval inside act() so GET /api/v1/profiles/default_voice/finetune resolves succeeded cleanly
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2600);
      });

      await waitFor(() => {
        expect(pollCount).toBeGreaterThanOrEqual(1);
        expect(
          screen.getByTestId('finetune-progress-box-default_voice'),
        ).toHaveTextContent('Epoch 8 / 8 · Progress 100% · Loss 0.3105');
        expect(
          screen.getByTestId('profile-likeness-tier-default_voice'),
        ).toHaveTextContent('Fine-Tuned on RTX 3090');
      });

      // Revert to Zero-Shot Base Model
      await user.click(
        screen.getByRole('button', { name: /Revert to Zero-Shot Base Model/i }),
      );

      await waitFor(() => {
        expect(capturedRevertPatch).toEqual({ use_finetuned_checkpoint: false });
        expect(store.getState().profiles.items[0]?.finetune_status).toBe('none');
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it('queues multiple diverse reference clips and High Likeness preset in EnrollmentTab and sends additional_samples + tuning in POST /api/v1/profiles', async () => {
    let capturedEnrollBody: Record<string, unknown> | null = null;

    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/profiles') && method === 'POST') {
        capturedEnrollBody = JSON.parse(String(init?.body ?? '{}')) as Record<
          string,
          unknown
        >;
        return jsonResponse(
          {
            id: '01933c89-0000-7000-8000-000000000009',
            slug: capturedEnrollBody.slug,
            display_name: capturedEnrollBody.display_name,
            avatar_url: null,
            reference_wav_path: `/data/profiles/${String(capturedEnrollBody.slug)}.wav`,
            reference_transcript_en: capturedEnrollBody.reference_transcript_en,
            preferred_engine: capturedEnrollBody.preferred_engine,
            default_target_language: capturedEnrollBody.default_target_language,
            owner_discord_user_id: 987654321,
            created_at: '2026-11-18T09:00:00Z',
            sample_count: 2,
            total_sample_duration_ms: 2000,
            tuning: capturedEnrollBody.tuning,
          },
          201,
        );
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();

    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    await user.click(
      screen.getByRole('button', { name: /2\. Voice Recording & Enrollment Studio/i }),
    );

    const makeWavFile = (name: string, freqHz: number) => {
      const sine = new Float32Array(24_000);
      for (let i = 0; i < sine.length; i += 1) {
        sine[i] = 0.4 * Math.sin((2 * Math.PI * freqHz * i) / 24_000);
      }
      const { wavBytes } = encodeMonoWav24k(sine, 24_000);
      const buf = new ArrayBuffer(wavBytes.byteLength);
      new Uint8Array(buf).set(wavBytes);
      return new File([buf], name, { type: 'audio/wav' });
    };

    // Clip 1: queue primary conversational clip
    await user.upload(
      screen.getByLabelText(/Upload Reference Audio File/i),
      makeWavFile('clip1.wav', 260),
    );
    await waitFor(() => {
      expect(screen.getByTestId('captured-audio-card')).toBeInTheDocument();
    });
    await user.click(
      screen.getByRole('button', { name: /\+ Add Clip to Profile Queue/i }),
    );

    expect(screen.getByTestId('enrollment-clip-queue')).toHaveTextContent('1 clip');

    // Clip 2: select Calm Narrative script (script index 2) and queue second clip
    const scriptButtons = screen.getAllByRole('button', {
      name: /Use This Script & Populate Transcript/i,
    });
    await user.click(scriptButtons[2]!);
    await user.upload(
      screen.getByLabelText(/Upload Reference Audio File/i),
      makeWavFile('clip2.wav', 310),
    );
    await waitFor(() => {
      expect(screen.getByTestId('captured-audio-card')).toBeInTheDocument();
    });
    await user.click(
      screen.getByRole('button', { name: /\+ Add Clip to Profile Queue/i }),
    );

    expect(screen.getByTestId('enrollment-clip-queue')).toHaveTextContent('2 clips');

    // Select High Likeness (Recommended) preset
    await user.click(
      screen.getByRole('button', { name: /High Likeness \(Recommended\)/i }),
    );

    const slugInput = screen.getByLabelText(/Profile Slug/i);
    await user.clear(slugInput);
    await user.type(slugInput, 'multisample_voice');

    await user.click(
      screen.getByRole('button', { name: /Enroll Voice Profile \(2 Reference Clips\)/i }),
    );

    await waitFor(() => {
      expect(store.getState().studio.selectedProfileSlug).toBe('multisample_voice');
    });

    expect(capturedEnrollBody).not.toBeNull();
    const sentEnroll = capturedEnrollBody as unknown as Record<string, unknown>;
    expect(sentEnroll['slug']).toBe('multisample_voice');
    const additional = sentEnroll['additional_samples'] as Array<
      Record<string, unknown>
    >;
    expect(additional).toHaveLength(1);
    expect(additional[0]?.['style_label']).toBe('calm');
      expect(sentEnroll['tuning']).toMatchObject({
      temperature: 0.58,
      top_p: 0.78,
      top_k: 45,
      repetition_penalty: 5.5,
    });
  });

  it('renders DesktopSetupPanel hardware tier and saves Discord credentials in standalone desktop mode', async () => {
    let savedDiscordBody: Record<string, unknown> | null = null;

    installBaseHandlers((url, method, init) => {
      if (url.endsWith('/api/v1/config') && method === 'GET') {
        return jsonResponse({
          ...MOCK_CONFIG,
          standalone_desktop_mode: true,
          discord_configured: false,
        });
      }
      if (url.endsWith('/api/v1/system/status') && method === 'GET') {
        return jsonResponse({
          standalone_desktop_mode: true,
          discord_configured: Boolean(savedDiscordBody),
          discord_application_id_str: '0',
          discord_owner_user_id_str: '1',
          user_install_url: null,
          bot_dm_url: null,
          studio_env_writable: true,
          hardware_specs: {
            os: 'windows',
            arch: 'x86_64',
            cpu_logical_cores: 16,
            system_ram_mib: 32768,
            free_disk_mib: 90000,
            gpus: [
              {
                index: 0,
                name: 'NVIDIA GeForce RTX 4070',
                vendor: 'nvidia',
                vram_total_mib: 12288,
                vram_free_mib: 11200,
                driver_version: '566.14',
              },
            ],
          },
          hardware_plan: {
            tier: 'mainstream_cuda',
            selected_gpu_index: 0,
            selected_gpu_name: 'NVIDIA GeForce RTX 4070',
            selected_gpu_vram_mib: 12288,
            whisper_use_gpu: true,
            whisper_model_filename: 'ggml-small.bin',
            llm_model_filename: 'Qwen3-8B-Q4_K_M.gguf',
            llm_gpu_layers: 99,
            prewarm_engines: ['xtts_v2'],
            estimated_download_mib: 7275,
            warnings: [],
          },
        });
      }
      if (url.endsWith('/api/v1/system/discord-config') && method === 'POST') {
        savedDiscordBody = JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>;
        return jsonResponse({
          saved: true,
          verified_bot_username: 'VoiceCloneBot',
          user_install_url:
            'https://discord.com/oauth2/authorize?client_id=111111111111111111&integration_type=1&scope=applications.commands',
          bot_dm_url:
            'https://discord.com/oauth2/authorize?client_id=111111111111111111&integration_type=0&scope=bot+applications.commands&permissions=274877910016',
        });
      }
      return null;
    });

    const user = userEvent.setup();
    const store = createAppStore();
    render(
      <Provider store={store}>
        <App />
      </Provider>,
    );

    const pairBtn = await screen.findByRole('button', { name: /Pair Discord Bot/i });
    await user.click(pairBtn);

    const panel = await screen.findByTestId('desktop-setup-panel');
    expect(panel).toHaveTextContent('mainstream_cuda');
    expect(panel).toHaveTextContent('NVIDIA GeForce RTX 4070 (12288 MiB)');

    await user.type(screen.getByLabelText(/Discord Application ID/i), '111111111111111111');
    await user.type(screen.getByLabelText(/Discord Bot Token/i), 'MTIz.bot-secret');
    await user.type(screen.getByLabelText(/Your Discord User ID/i), '222222222222222222');
    await user.click(screen.getByRole('button', { name: /Save Discord Config & Start Bot/i }));

    const receipt = await screen.findByTestId('discord-save-receipt');
    expect(receipt).toHaveTextContent('verified @VoiceCloneBot');
    expect(savedDiscordBody).toMatchObject({
      discord_application_id: '111111111111111111',
      discord_owner_user_id: '222222222222222222',
      verify_online: true,
    });
  });
});
