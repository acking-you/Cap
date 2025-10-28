import { Button } from "@cap/ui-solid";
import { Select as KSelect } from "@kobalte/core/select";
import { makePersisted } from "@solid-primitives/storage";
import {
	createMutation,
	createQuery,
	keepPreviousData,
} from "@tanstack/solid-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { cx } from "cva";
import {
	createEffect,
	createRoot,
	createSignal,
	For,
	type JSX,
	Match,
	on,
	Show,
	Switch,
	type ValidComponent,
} from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import toast from "solid-toast";
import { trackEvent } from "~/utils/analytics";
import { exportVideo } from "~/utils/export";
import {
	commands,
	type ExportCompression,
	type ExportSettings,
	events,
	type FramesRendered,
} from "~/utils/tauri";
import { type RenderState, useEditorContext } from "./context";
import { RESOLUTION_OPTIONS } from "./Header";
import {
	Dialog,
	DialogContent,
	MenuItem,
	MenuItemList,
	PopperContent,
	topSlideAnimateClasses,
} from "./ui";

class SilentError extends Error {}

export const COMPRESSION_OPTIONS: Array<{
	label: string;
	value: ExportCompression;
}> = [
	{ label: "H.265 Lossless (GPU)", value: "H265Lossless" },
	{ label: "H.264 Lossless (CPU)", value: "H264Lossless" },
	{ label: "Near-Lossless", value: "NearLossless" },
	{ label: "High Quality", value: "HighQuality" },
	{ label: "Minimal", value: "Minimal" },
	{ label: "Social Media", value: "Social" },
	{ label: "Web", value: "Web" },
	{ label: "Potato", value: "Potato" },
];

export const FPS_OPTIONS = [
	{ label: "Original FPS", value: 0 },
	{ label: "15 FPS", value: 15 },
	{ label: "30 FPS", value: 30 },
	{ label: "60 FPS", value: 60 },
	{ label: "90 FPS", value: 90 },
	{ label: "120 FPS", value: 120 },
	{ label: "144 FPS", value: 144 },
	{ label: "165 FPS", value: 165 },
	{ label: "180 FPS", value: 180 },
	{ label: "240 FPS", value: 240 },
] satisfies Array<{ label: string; value: number }>;

export const GIF_FPS_OPTIONS = [
	{ label: "Original FPS", value: 0 },
	{ label: "10 FPS", value: 10 },
	{ label: "15 FPS", value: 15 },
	{ label: "20 FPS", value: 20 },
	{ label: "25 FPS", value: 25 },
	{ label: "30 FPS", value: 30 },
] satisfies Array<{ label: string; value: number }>;

export const EXPORT_TO_OPTIONS = [
	{
		label: "File",
		value: "file",
		icon: <IconCapFile class="text-gray-12 size-3.5" />,
	},
	{
		label: "Clipboard",
		value: "clipboard",
		icon: <IconCapCopy class="text-gray-12 size-3.5" />,
	},
] as const;

type ExportFormat = ExportSettings["format"];

export const FORMAT_OPTIONS = [
	{ label: "MP4", value: "Mp4" },
	{ label: "GIF", value: "Gif" },
] as { label: string; value: ExportFormat; disabled?: boolean }[];

type ExportToOption = (typeof EXPORT_TO_OPTIONS)[number]["value"];

interface Settings {
	format: ExportFormat;
	fps: number;
	exportTo: ExportToOption;
	resolution: { label: string; value: string; width: number; height: number };
	compression: ExportCompression;
	gpuAsyncDepth: number;
	gpuDelay: number;
	gpuRcLookahead: number;
	showAdvanced: boolean;
}
export function ExportDialog() {
	const {
		dialog,
		setDialog,
		editorInstance,
		setExportState,
		exportState,
		meta,
		refetchMeta,
	} = useEditorContext();

	const [settings, setSettings] = makePersisted(
		createStore<Settings>({
			format: "Mp4",
			fps: 0,
			exportTo: "file",
			resolution: { label: "Original", value: "original", width: 0, height: 0 },
			compression: "HighQuality",
			gpuAsyncDepth: 32,
			gpuDelay: 4,
			gpuRcLookahead: 16,
			showAdvanced: false,
		}),
		{ name: "export_settings" },
	);

	if (!["Mp4", "Gif"].includes(settings.format)) setSettings("format", "Mp4");

	const exportWithSettings = (onProgress: (progress: FramesRendered) => void) => {
		const originalDisplay = editorInstance.recordings.segments[0]?.display;
		const actualFps = settings.fps === 0 ? (originalDisplay?.fps ?? 30) : settings.fps;
		const actualWidth = settings.resolution.value === "original"
			? (originalDisplay?.width ?? 1280)
			: settings.resolution.width;
		const actualHeight = settings.resolution.value === "original"
			? (originalDisplay?.height ?? 720)
			: settings.resolution.height;

		return exportVideo(
			projectPath,
			settings.format === "Mp4"
				? {
						format: "Mp4",
						fps: actualFps,
						resolution_base: {
							x: actualWidth,
							y: actualHeight,
						},
						compression: settings.compression,
						gpu_async_depth: settings.gpuAsyncDepth,
						gpu_delay: settings.gpuDelay,
						gpu_rc_lookahead: settings.gpuRcLookahead,
					}
				: {
						format: "Gif",
						fps: actualFps,
						resolution_base: {
							x: actualWidth,
							y: actualHeight,
						},
						quality: null,
					},
			onProgress,
		);
	};

	const [outputPath, setOutputPath] = createSignal<string | null>(null);

	const selectedStyle = "bg-gray-7";

	const projectPath = editorInstance.path;

	const exportEstimates = createQuery(() => {
		const originalDisplay = editorInstance.recordings.segments[0]?.display;
		const actualFps = settings.fps === 0 ? (originalDisplay?.fps ?? 30) : settings.fps;
		const actualWidth = settings.resolution.value === "original"
			? (originalDisplay?.width ?? 1280)
			: settings.resolution.width;
		const actualHeight = settings.resolution.value === "original"
			? (originalDisplay?.height ?? 720)
			: settings.resolution.height;

		return {
			placeholderData: keepPreviousData,
			queryKey: [
				"exportEstimates",
				{
					resolution: {
						x: actualWidth,
						y: actualHeight,
					},
					fps: actualFps,
				},
			] as const,
			queryFn: ({ queryKey: [_, { resolution, fps }] }) =>
				commands.getExportEstimates(projectPath, resolution, fps),
		};
	});

	const exportButtonIcon: Record<"file" | "clipboard", JSX.Element> = {
		file: <IconCapFile class="text-gray-1 size-3.5" />,
		clipboard: <IconCapCopy class="text-gray-1 size-3.5" />,
	};

	const copy = createMutation(() => ({
		mutationFn: async () => {
			if (exportState.type !== "idle") return;
			setExportState(reconcile({ action: "copy", type: "starting" }));

			const outputPath = await exportWithSettings((progress) => {
				setExportState({ type: "rendering", progress });
			});

			setExportState({ type: "copying" });

			await commands.copyVideoToClipboard(outputPath);
		},
		onError: (error) => {
			commands.globalMessageDialog(
				error instanceof Error ? error.message : "Failed to copy recording",
			);
			setExportState(reconcile({ type: "idle" }));
		},
		onSuccess() {
			setExportState({ type: "done" });

			if (dialog().open) {
				createRoot((dispose) => {
					createEffect(
						on(
							() => dialog().open,
							() => {
								dispose();
							},
							{ defer: true },
						),
					);
				});
			} else
				toast.success(
					`${
						settings.format === "Gif" ? "GIF" : "Recording"
					} exported to clipboard`,
				);
		},
	}));

	const save = createMutation(() => ({
		mutationFn: async () => {
			if (exportState.type !== "idle") return;

			const extension = settings.format === "Gif" ? "gif" : "mp4";
			const savePath = await saveDialog({
				filters: [
					{
						name: `${extension.toUpperCase()} filter`,
						extensions: [extension],
					},
				],
				defaultPath: `~/Desktop/${meta().prettyName}.${extension}`,
			});
			if (!savePath) {
				setExportState(reconcile({ type: "idle" }));
				return;
			}

			setExportState(reconcile({ action: "save", type: "starting" }));

			setOutputPath(savePath);

			trackEvent("export_started", {
				resolution: settings.resolution,
				fps: settings.fps,
				path: savePath,
			});

			const videoPath = await exportWithSettings((progress) => {
				setExportState({ type: "rendering", progress });
			});

			setExportState({ type: "copying" });

			await commands.copyFileToPath(videoPath, savePath);

			setExportState({ type: "done" });
		},
		onError: (error) => {
			commands.globalMessageDialog(
				error instanceof Error
					? error.message
					: `Failed to export recording: ${error}`,
			);
			setExportState({ type: "idle" });
		},
		onSuccess() {
			if (dialog().open) {
				createRoot((dispose) => {
					createEffect(
						on(
							() => dialog().open,
							() => {
								dispose();
							},
							{ defer: true },
						),
					);
				});
			} else
				toast.success(
					`${settings.format === "Gif" ? "GIF" : "Recording"} exported to file`,
				);
		},
	}));


	return (
		<>
			<Show when={exportState.type === "idle"}>
				<DialogContent
					title="Export Cap"
					confirm={
						<Button
							class="flex gap-1.5 items-center"
							variant="dark"
							onClick={() => {
								if (settings.exportTo === "file") save.mutate();
								else copy.mutate();
							}}
						>
							Export to
							{exportButtonIcon[settings.exportTo]}
						</Button>
					}
					leftFooterContent={
						<div>
							<Show when={exportEstimates.data}>
								{(est) => (
									<div
										class={cx(
											"flex overflow-hidden z-40 justify-between items-center max-w-full text-xs font-medium transition-all pointer-events-none",
										)}
									>
										<p class="flex gap-4 items-center">
											<span class="flex items-center text-gray-12">
												<IconCapCamera class="w-[14px] h-[14px] mr-1.5 text-gray-12" />
												{(() => {
													const totalSeconds = Math.round(
														est().duration_seconds,
													);
													const hours = Math.floor(totalSeconds / 3600);
													const minutes = Math.floor(
														(totalSeconds % 3600) / 60,
													);
													const seconds = totalSeconds % 60;

													if (hours > 0) {
														return `${hours}:${minutes
															.toString()
															.padStart(2, "0")}:${seconds
															.toString()
															.padStart(2, "0")}`;
													}
													return `${minutes}:${seconds
														.toString()
														.padStart(2, "0")}`;
												})()}
											</span>
											<span class="flex items-center text-gray-12">
												<IconLucideMonitor class="w-[14px] h-[14px] mr-1.5 text-gray-12" />
												{(() => {
													const originalDisplay = editorInstance.recordings.segments[0]?.display;
													const actualWidth = settings.resolution.value === "original"
														? (originalDisplay?.width ?? 1280)
														: settings.resolution.width;
													const actualHeight = settings.resolution.value === "original"
														? (originalDisplay?.height ?? 720)
														: settings.resolution.height;
													return `${actualWidth}×${actualHeight}`;
												})()}
											</span>
											<span class="flex items-center text-gray-12">
												<IconLucideHardDrive class="w-[14px] h-[14px] mr-1.5 text-gray-12" />
												{est().estimated_size_mb.toFixed(2)} MB
											</span>
											<span class="flex items-center text-gray-12">
												<IconLucideClock class="w-[14px] h-[14px] mr-1.5 text-gray-12" />
												{(() => {
													const totalSeconds = Math.round(
														est().estimated_time_seconds,
													);
													const hours = Math.floor(totalSeconds / 3600);
													const minutes = Math.floor(
														(totalSeconds % 3600) / 60,
													);
													const seconds = totalSeconds % 60;

													if (hours > 0) {
														return `~${hours}:${minutes
															.toString()
															.padStart(2, "0")}:${seconds
															.toString()
															.padStart(2, "0")}`;
													}
													return `~${minutes}:${seconds
														.toString()
														.padStart(2, "0")}`;
												})()}
											</span>
										</p>
									</div>
								)}
							</Show>
						</div>
					}
				>
					<div class="flex flex-wrap gap-3">
						{/* Export to */}
						<div class="flex-1 p-4 rounded-xl dark:bg-gray-2 bg-gray-3">
							<div class="flex flex-col gap-3">
								<h3 class="text-gray-12">Export to</h3>
								<div class="flex gap-2">
									<For each={EXPORT_TO_OPTIONS}>
										{(option) => (
											<Button
												onClick={() => setSettings("exportTo", option.value)}
												data-selected={settings.exportTo === option.value}
												class="flex flex-1 gap-2 items-center text-nowrap"
												variant="gray"
											>
												{option.icon}
												{option.label}
											</Button>
										)}
									</For>
								</div>
							</div>
						</div>
						{/* Format */}
						<div class="p-4 rounded-xl dark:bg-gray-2 bg-gray-3">
							<div class="flex flex-col gap-3">
								<h3 class="text-gray-12">Format</h3>
								<div class="flex flex-row gap-2">
									<For each={FORMAT_OPTIONS}>
										{(option) => (
											<Button
												variant="gray"
												onClick={() => {
													setSettings(
														produce((newSettings) => {
															newSettings.format = option.value as ExportFormat;

															if (
																option.value === "Gif" &&
																!(
																	settings.resolution.value === "720p" ||
																	settings.resolution.value === "1080p"
																)
															)
																newSettings.resolution = {
																	...RESOLUTION_OPTIONS._720p,
																};

															if (
																option.value === "Gif" &&
																!GIF_FPS_OPTIONS.some(
																	(v) => v.value === settings.fps,
																)
															)
																newSettings.fps = 15;

															if (
																option.value === "Mp4" &&
																!FPS_OPTIONS.some(
																	(v) => v.value === settings.fps,
																)
															)
																newSettings.fps = 30;
														}),
													);
												}}
												autofocus={false}
												data-selected={settings.format === option.value}
											>
												{option.label}
											</Button>
										)}
									</For>
								</div>
							</div>
						</div>
						{/* Frame rate */}
						<div class="overflow-hidden relative p-4 rounded-xl dark:bg-gray-2 bg-gray-3">
							<div class="flex flex-col gap-3">
								<h3 class="text-gray-12">Frame rate</h3>
								<KSelect<{ label: string; value: number }>
									options={
										settings.format === "Gif" ? GIF_FPS_OPTIONS : FPS_OPTIONS
									}
									optionValue="value"
									optionTextValue="label"
									placeholder="Select FPS"
									value={(settings.format === "Gif"
										? GIF_FPS_OPTIONS
										: FPS_OPTIONS
									).find((opt) => opt.value === settings.fps)}
									onChange={(option) => {
										const value =
											option?.value ?? (settings.format === "Gif" ? 10 : 30);
										trackEvent("export_fps_changed", {
											fps: value,
										});
										setSettings("fps", value);
									}}
									itemComponent={(props) => (
										<MenuItem<typeof KSelect.Item>
											as={KSelect.Item}
											item={props.item}
										>
											<KSelect.ItemLabel class="flex-1">
												{props.item.rawValue.label}
											</KSelect.ItemLabel>
										</MenuItem>
									)}
								>
									<KSelect.Trigger class="flex flex-row gap-2 items-center px-3 w-full h-10 rounded-xl transition-colors dark:bg-gray-3 bg-gray-4 disabled:text-gray-11">
										<KSelect.Value<
											(typeof FPS_OPTIONS)[number]
										> class="flex-1 text-sm text-left truncate tabular-nums text-[--gray-500]">
											{(state) => <span>{state.selectedOption()?.label}</span>}
										</KSelect.Value>
										<KSelect.Icon<ValidComponent>
											as={(props) => (
												<IconCapChevronDown
													{...props}
													class="size-4 shrink-0 transform transition-transform ui-expanded:rotate-180 text-[--gray-500]"
												/>
											)}
										/>
									</KSelect.Trigger>
									<KSelect.Portal>
										<PopperContent<typeof KSelect.Content>
											as={KSelect.Content}
											class={cx(topSlideAnimateClasses, "z-50")}
										>
											<MenuItemList<typeof KSelect.Listbox>
												class="max-h-32 custom-scroll"
												as={KSelect.Listbox}
											/>
										</PopperContent>
									</KSelect.Portal>
								</KSelect>
							</div>
						</div>
						{/* Compression */}
						<div class="p-4 rounded-xl dark:bg-gray-2 bg-gray-3">
							<div class="flex flex-col gap-3">
								<h3 class="text-gray-12">Compression</h3>
								<div class="flex flex-wrap gap-2">
									<For each={COMPRESSION_OPTIONS}>
										{(option) => (
											<Button
												onClick={() => {
													setSettings(
														"compression",
														option.value as ExportCompression,
													);
												}}
												variant="gray"
												data-selected={settings.compression === option.value}
												class="flex-1 min-w-[90px]"
											>
												{option.label}
											</Button>
										)}
									</For>
								</div>
							</div>
						</div>
						{/* Resolution */}
						<div class="flex-1 p-4 rounded-xl dark:bg-gray-2 bg-gray-3">
							<div class="flex flex-col gap-3">
								<h3 class="text-gray-12">Resolution</h3>
								<div class="flex gap-2">
									<For
										each={
											settings.format === "Gif"
												? [RESOLUTION_OPTIONS._720p, RESOLUTION_OPTIONS._1080p]
												: [
														RESOLUTION_OPTIONS._original,
														RESOLUTION_OPTIONS._720p,
														RESOLUTION_OPTIONS._1080p,
														RESOLUTION_OPTIONS._1440p,
														RESOLUTION_OPTIONS._4k,
													]
										}
									>
										{(option) => (
											<Button
												data-selected={
													settings.resolution.value === option.value
												}
												class="flex-1"
												variant="gray"
												onClick={() => setSettings("resolution", option)}
											>
												{option.label}
											</Button>
										)}
									</For>
								</div>
							</div>
						</div>
						{/* Advanced Settings (MP4 only) */}
						<Show when={settings.format === "Mp4"}>
							<div class="w-full p-4 rounded-xl dark:bg-gray-2 bg-gray-3">
								<div class="flex flex-col gap-3">
									<div class="flex justify-between items-center">
										<h3 class="text-gray-12">GPU Optimization (Advanced)</h3>
										<Button
											variant="gray"
											class="text-xs"
											onClick={() => setSettings("showAdvanced", !settings.showAdvanced)}
										>
											{settings.showAdvanced ? "Hide" : "Show"}
										</Button>
									</div>
									<Show when={settings.showAdvanced}>
										<div class="flex flex-col gap-4 pt-2">
											{/* Async Depth */}
											<div class="flex flex-col gap-2">
												<div class="flex justify-between items-center">
													<label class="text-sm text-gray-11">Async Depth</label>
													<input
														type="number"
														min="1"
														max="64"
														value={settings.gpuAsyncDepth}
														onChange={(e) => setSettings("gpuAsyncDepth", Number.parseInt(e.target.value) || 32)}
														class="px-2 py-1 w-16 text-sm rounded bg-gray-3 dark:bg-gray-4 text-gray-12"
													/>
												</div>
												<input
													type="range"
													min="1"
													max="64"
													value={settings.gpuAsyncDepth}
													onChange={(e) => setSettings("gpuAsyncDepth", Number.parseInt(e.target.value))}
													class="w-full"
												/>
												<p class="text-xs text-gray-11">GPU parallel frame processing (higher = faster, more VRAM)</p>
											</div>
											{/* Delay */}
											<div class="flex flex-col gap-2">
												<div class="flex justify-between items-center">
													<label class="text-sm text-gray-11">Frame Delay</label>
													<input
														type="number"
														min="0"
														max="16"
														value={settings.gpuDelay}
														onChange={(e) => setSettings("gpuDelay", Number.parseInt(e.target.value) || 4)}
														class="px-2 py-1 w-16 text-sm rounded bg-gray-3 dark:bg-gray-4 text-gray-12"
													/>
												</div>
												<input
													type="range"
													min="0"
													max="16"
													value={settings.gpuDelay}
													onChange={(e) => setSettings("gpuDelay", Number.parseInt(e.target.value))}
													class="w-full"
												/>
												<p class="text-xs text-gray-11">Frame reordering for better compression</p>
											</div>
											{/* RC Lookahead */}
											<div class="flex flex-col gap-2">
												<div class="flex justify-between items-center">
													<label class="text-sm text-gray-11">Rate Control Lookahead</label>
													<input
														type="number"
														min="0"
														max="32"
														value={settings.gpuRcLookahead}
														onChange={(e) => setSettings("gpuRcLookahead", Number.parseInt(e.target.value) || 16)}
														class="px-2 py-1 w-16 text-sm rounded bg-gray-3 dark:bg-gray-4 text-gray-12"
													/>
												</div>
												<input
													type="range"
													min="0"
													max="32"
													value={settings.gpuRcLookahead}
													onChange={(e) => setSettings("gpuRcLookahead", Number.parseInt(e.target.value))}
													class="w-full"
												/>
												<p class="text-xs text-gray-11">Lookahead frames for bitrate optimization</p>
											</div>
										</div>
									</Show>
								</div>
							</div>
						</Show>
					</div>
				</DialogContent>
			</Show>
			<Show when={exportState.type !== "idle" && exportState} keyed>
				{(exportState) => {
					const [copyPressed, setCopyPressed] = createSignal(false);
					const [clipboardCopyPressed, setClipboardCopyPressed] =
						createSignal(false);
					const [showCompletionScreen, setShowCompletionScreen] = createSignal(
						exportState.type === "done" && exportState.action === "save",
					);

					createEffect(() => {
						if (exportState.type === "done" && exportState.action === "save") {
							setShowCompletionScreen(true);
						}
					});

					return (
						<>
							<Dialog.Header>
								<div class="flex justify-between items-center w-full">
									<span class="text-gray-12">Export</span>
									<div
										onClick={() => setDialog((d) => ({ ...d, open: false }))}
										class="flex justify-center items-center p-1 rounded-full transition-colors cursor-pointer hover:bg-gray-3"
									>
										<IconCapCircleX class="text-gray-12 size-4" />
									</div>
								</div>
							</Dialog.Header>
							<Dialog.Content class="text-gray-12">
								<div class="relative z-10 px-5 py-4 mx-auto space-y-6 w-full text-center">
									<Switch>
										<Match
											when={exportState.action === "copy" && exportState}
											keyed
										>
											{(copyState) => (
												<div class="flex flex-col gap-4 justify-center items-center h-full">
													<h1 class="text-lg font-medium text-gray-12">
														{copyState.type === "starting"
															? "Preparing..."
															: copyState.type === "rendering"
																? settings.format === "Gif"
																	? "Rendering GIF..."
																	: "Rendering video..."
																: copyState.type === "copying"
																	? "Copying to clipboard..."
																	: "Copied to clipboard"}
													</h1>
													<Show
														when={
															(copyState.type === "rendering" ||
																copyState.type === "starting") &&
															copyState
														}
														keyed
													>
														{(copyState) => (
															<RenderProgress
																state={copyState}
																format={settings.format}
															/>
														)}
													</Show>
												</div>
											)}
										</Match>
										<Match
											when={exportState.action === "save" && exportState}
											keyed
										>
											{(saveState) => (
												<div class="flex flex-col gap-4 justify-center items-center h-full">
													<Show
														when={
															showCompletionScreen() &&
															saveState.type === "done"
														}
														fallback={
															<>
																<h1 class="text-lg font-medium text-gray-12">
																	{saveState.type === "starting"
																		? "Preparing..."
																		: saveState.type === "rendering"
																			? settings.format === "Gif"
																				? "Rendering GIF..."
																				: "Rendering video..."
																			: saveState.type === "copying"
																				? "Exporting to file..."
																				: "Export completed"}
																</h1>
																<Show
																	when={
																		(saveState.type === "rendering" ||
																			saveState.type === "starting") &&
																		saveState
																	}
																	keyed
																>
																	{(copyState) => (
																		<RenderProgress
																			state={copyState}
																			format={settings.format}
																		/>
																	)}
																</Show>
															</>
														}
													>
														<div class="flex flex-col gap-6 items-center duration-500 animate-in fade-in">
															<div class="flex flex-col gap-3 items-center">
																<div class="flex justify-center items-center mb-2 rounded-full bg-gray-12 size-10">
																	<IconLucideCheck class="text-gray-1 size-5" />
																</div>
																<div class="flex flex-col gap-1 items-center">
																	<h1 class="text-xl font-medium text-gray-12">
																		Export Completed
																	</h1>
																	<p class="text-sm text-gray-11">
																		Your{" "}
																		{settings.format === "Gif"
																			? "GIF"
																			: "video"}{" "}
																		has successfully been exported
																	</p>
																</div>
															</div>
														</div>
													</Show>
												</div>
											)}
										</Match>
									</Switch>
								</div>
							</Dialog.Content>
							<Dialog.Footer>

								<Show
									when={
										exportState.action === "save" && exportState.type === "done"
									}
								>
									<div class="flex gap-4 w-full">
										<Button
											variant="dark"
											class="flex gap-2 items-center"
											onClick={() => {
												const path = outputPath();
												if (path) {
													commands.openFilePath(path);
												}
											}}
										>
											<IconCapFile class="size-4" />
											Open File
										</Button>
										<Button
											variant="dark"
											class="flex gap-2 items-center"
											onClick={async () => {
												const path = outputPath();
												if (path) {
													setClipboardCopyPressed(true);
													setTimeout(() => {
														setClipboardCopyPressed(false);
													}, 2000);
													await commands.copyVideoToClipboard(path);
													toast.success(
														`${
															settings.format === "Gif" ? "GIF" : "Video"
														} copied to clipboard`,
													);
												}
											}}
										>
											{!clipboardCopyPressed() ? (
												<IconCapCopy class="size-4" />
											) : (
												<IconLucideCheck class="size-4 svgpathanimation" />
											)}
											Copy to Clipboard
										</Button>
									</div>
								</Show>
							</Dialog.Footer>
						</>
					);
				}}
			</Show>
		</>
	);
}

function RenderProgress(props: { state: RenderState; format?: ExportFormat }) {
	return (
		<ProgressView
			amount={
				props.state.type === "rendering"
					? (props.state.progress.renderedCount /
							props.state.progress.totalFrames) *
						100
					: 0
			}
			label={
				props.state.type === "rendering"
					? `Rendering ${props.format === "Gif" ? "GIF" : "video"} (${
							props.state.progress.renderedCount
						}/${props.state.progress.totalFrames} frames)`
					: "Preparing to render..."
			}
		/>
	);
}

function ProgressView(props: { amount: number; label?: string }) {
	return (
		<>
			<div class="w-full bg-gray-3 rounded-full h-2.5">
				<div
					class="bg-blue-9 h-2.5 rounded-full"
					style={{ width: `${props.amount}%` }}
				/>
			</div>
			<p class="text-xs tabular-nums">{props.label}</p>
		</>
	);
}
