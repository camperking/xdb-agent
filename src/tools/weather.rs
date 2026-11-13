use rig::tool::{Tool, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize, Serialize)]
pub struct GetWeatherArgs {
    pub location: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct WeatherError(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetWeather;

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    results: Option<Vec<GeocodingResult>>,
}

#[derive(Debug, Deserialize)]
struct GeocodingResult {
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
    admin1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForecastResponse {
    current: Option<CurrentWeather>,
}

#[derive(Debug, Deserialize)]
struct CurrentWeather {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: u64,
    wind_speed_10m: f64,
    weather_code: u64,
}

impl Tool for GetWeather {
    const NAME: &'static str = "get_weather";

    type Error = WeatherError;
    type Args = GetWeatherArgs;
    type Output = String;

    fn description(&self) -> String {
        "Ruft das aktuelle Wetter für einen Ort über Open-Meteo ab und gibt eine kompakte Zusammenfassung zurück.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "Ort oder Region, z. B. 'Berlin', 'New York' oder '48.14,11.58'"
                }
            },
            "required": ["location"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let location = args.location.trim();
        if location.is_empty() {
            return Err(WeatherError("location must not be empty".to_string()));
        }

        let client = reqwest::Client::new();

        let geocoding_response = client
            .get("https://geocoding-api.open-meteo.com/v1/search")
            .query(&[
                ("name", location),
                ("count", "1"),
                ("language", "en"),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|error| WeatherError(format!("Failed to geocode location: {error}")))?;

        if !geocoding_response.status().is_success() {
            return Err(WeatherError(format!(
                "Open-Meteo geocoding request failed with status {}",
                geocoding_response.status()
            )));
        }

        let geocoding = geocoding_response
            .json::<GeocodingResponse>()
            .await
            .map_err(|error| {
                WeatherError(format!("Failed to parse geocoding response: {error}"))
            })?;

        let match_result = geocoding
            .results
            .and_then(|mut results| results.drain(..).next())
            .ok_or_else(|| WeatherError(format!("No location found for '{location}'")))?;

        let weather_response = client
            .get("https://api.open-meteo.com/v1/forecast")
            .query(&[
                ("latitude", match_result.latitude.to_string()),
                ("longitude", match_result.longitude.to_string()),
                (
                    "current",
                    "temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code".to_string(),
                ),
                ("timezone", "auto".to_string()),
            ])
            .send()
            .await
            .map_err(|error| WeatherError(format!("Failed to fetch weather data: {error}")))?;

        if !weather_response.status().is_success() {
            return Err(WeatherError(format!(
                "Open-Meteo forecast request failed with status {}",
                weather_response.status()
            )));
        }

        let forecast = weather_response
            .json::<ForecastResponse>()
            .await
            .map_err(|error| WeatherError(format!("Failed to parse weather response: {error}")))?;

        let current = forecast.current.ok_or_else(|| {
            WeatherError("Open-Meteo response missing current weather data".to_string())
        })?;

        let mut place_parts = vec![match_result.name];
        if let Some(admin1) = match_result.admin1 {
            if !admin1.is_empty() {
                place_parts.push(admin1);
            }
        }
        if let Some(country) = match_result.country {
            if !country.is_empty() {
                place_parts.push(country);
            }
        }

        let place = place_parts.join(", ");
        let condition = weather_code_description(current.weather_code);

        Ok(format!(
            "{}: {:.1}°C (feels {:.1}°C), humidity {}%, wind {:.1} km/h, {}",
            place,
            current.temperature_2m,
            current.apparent_temperature,
            current.relative_humidity_2m,
            current.wind_speed_10m,
            condition
        ))
    }
}

fn weather_code_description(code: u64) -> &'static str {
    match code {
        0 => "clear sky",
        1 | 2 | 3 => "partly cloudy",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 | 63 | 65 => "rain",
        66 | 67 => "freezing rain",
        71 | 73 | 75 | 77 => "snow",
        80 | 81 | 82 => "rain showers",
        85 | 86 => "snow showers",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown conditions",
    }
}
