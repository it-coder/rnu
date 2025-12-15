use colored::*;
use indicatif::{ProgressBar, ProgressStyle};

use std::time::{Duration, Instant};

pub struct UploadProgress {
    progress_bar: ProgressBar,
    start_time: Instant,
    total_packages: u64,
    uploaded: u64,
    skipped: u64,
    errors: u64,
}

impl UploadProgress {
    pub fn new(total_packages: u64) -> Self {
        let progress_bar = ProgressBar::new(total_packages);

        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({percent}%) {msg}"
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("#>-")
        );

        progress_bar.set_message("Starting upload...");

        Self {
            progress_bar,
            start_time: Instant::now(),
            total_packages,
            uploaded: 0,
            skipped: 0,
            errors: 0,
        }
    }

    pub fn set_message(&self, message: String) {
        self.progress_bar.set_message(message);
    }

    pub fn increment_uploaded(&mut self, package_name: &str, package_version: &str) {
        self.uploaded += 1;
        self.progress_bar.inc(1);
        self.update_message(package_name, package_version, "uploaded");
    }

    pub fn increment_skipped(&mut self, package_name: &str, package_version: &str) {
        self.skipped += 1;
        self.progress_bar.inc(1);
        self.update_message(package_name, package_version, "skipped");
    }

    pub fn increment_error(&mut self, package_name: &str, package_version: &str) {
        self.errors += 1;
        self.progress_bar.inc(1);
        self.update_message(package_name, package_version, "error");
    }

    fn update_message(&self, package_name: &str, package_version: &str, status: &str) {
        let elapsed = self.start_time.elapsed();
        let rate = if elapsed.as_secs() > 0 {
            (self.uploaded + self.skipped + self.errors) as f64 / elapsed.as_secs() as f64
        } else {
            0.0
        };

        let message = format!(
            "{}@{} ({}) - {:.1} pkg/s",
            package_name, package_version, status, rate
        );

        self.progress_bar.set_message(message);
    }

    pub fn finish(&self) {
        let elapsed = self.start_time.elapsed();
        let _total_processed = self.uploaded + self.skipped + self.errors;

        self.progress_bar.finish_with_message(format!(
            "Completed in {} - {} uploaded, {} skipped, {} errors",
            format_duration(elapsed),
            self.uploaded,
            self.skipped,
            self.errors
        ));
    }

    pub fn finish_with_summary(&self) {
        self.finish();
        self.print_summary();
    }

    pub fn print_summary(&self) {
        let elapsed = self.start_time.elapsed();

        println!("\n{}", "Upload Summary:".bold());
        println!("  {}: {}", "Total packages".bold(), self.total_packages);
        println!("  {}: {}", "Uploaded".green().bold(), self.uploaded);

        if self.skipped > 0 {
            println!("  {}: {}", "Skipped".yellow().bold(), self.skipped);
        }

        if self.errors > 0 {
            println!("  {}: {}", "Errors".red().bold(), self.errors);
        }

        println!("  {}: {}", "Time elapsed".bold(), format_duration(elapsed));

        if elapsed.as_secs() > 0 {
            let rate =
                (self.uploaded + self.skipped + self.errors) as f64 / elapsed.as_secs() as f64;
            println!("  {}: {:.2} packages/second", "Average rate".bold(), rate);
        }
    }

    pub fn get_stats(&self) -> (u64, u64, u64) {
        (self.uploaded, self.skipped, self.errors)
    }
}

pub struct SimpleProgress {
    current: usize,
    total: usize,
    prefix: String,
}

impl SimpleProgress {
    pub fn new(total: usize, prefix: String) -> Self {
        Self {
            current: 0,
            total,
            prefix,
        }
    }

    pub fn update(&mut self, current: usize, message: &str) {
        self.current = current;
        let percentage = if self.total > 0 {
            (current as f64 / self.total as f64 * 100.0) as usize
        } else {
            0
        };

        print!(
            "\r{} [{}/{}] ({}%) {}",
            self.prefix, current, self.total, percentage, message
        );

        use std::io::{self, Write};
        io::stdout().flush().unwrap_or(());
    }

    pub fn finish(&self, message: &str) {
        println!("\r{} Complete: {}", self.prefix, message);
    }
}

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();

    if total_seconds < 60 {
        format!("{}s", total_seconds)
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        if seconds == 0 {
            format!("{}m", minutes)
        } else {
            format!("{}m {}s", minutes, seconds)
        }
    } else {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        if minutes == 0 && seconds == 0 {
            format!("{}h", hours)
        } else if seconds == 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}h {}m {}s", hours, minutes, seconds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0s");
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(60)), "1m");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h");
        assert_eq!(format_duration(Duration::from_secs(3660)), "1h 1m");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m 1s");
    }

    #[test]
    fn test_upload_progress_creation() {
        let progress = UploadProgress::new(100);
        let (uploaded, skipped, errors) = progress.get_stats();

        assert_eq!(uploaded, 0);
        assert_eq!(skipped, 0);
        assert_eq!(errors, 0);
    }
}
