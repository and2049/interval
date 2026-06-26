import type { DerivedMetric, DriverSnapshot } from "../../../shared/types/api";

export interface DerivedMetricDisplay {
  driver: string;
  label: string;
  value: string;
  trend: DerivedMetric["trend"];
}

export function derivedMetricRows(
  metrics: DerivedMetric[],
  timingRows: DriverSnapshot[]
): DerivedMetricDisplay[] {
  const driverCodeByNumber = new Map(
    timingRows.map((row) => [row.driver.driver_number, row.driver.code])
  );

  return metrics.map((metric) => ({
    driver:
      metric.driver_number == null
        ? "--"
        : driverCodeByNumber.get(metric.driver_number) ?? metric.driver_number.toString(),
    label: metric.label,
    value: metric.value,
    trend: metric.trend
  }));
}
