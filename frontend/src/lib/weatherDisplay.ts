import type { ReplayWeatherSection } from "../../../shared/types/api";
import { formatPercent, formatSpeed, formatTemperature } from "./formatters";

export interface WeatherMetricDisplay {
  label: string;
  value: string;
}

export function weatherMetrics(weather: ReplayWeatherSection): WeatherMetricDisplay[] {
  return [
    { label: "Air", value: formatTemperature(weather.sample?.air_temp) },
    { label: "Track", value: formatTemperature(weather.sample?.track_temp) },
    { label: "Humidity", value: formatPercent(weather.sample?.humidity) },
    { label: "Wind", value: formatSpeed(weather.sample?.wind_speed) }
  ];
}

export function hasWeatherSample(weather: ReplayWeatherSection): boolean {
  return weather.sample != null;
}
