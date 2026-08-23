use app_core::{
    domain::object::{BoundingBox, Object, ObjectClass},
    traits::object_detector::ObjectDetector,
};
use tracing::{debug, info};
use ultralytics_inference::YOLOModel;

pub struct UltralyticsDetector {
    model: YOLOModel,
}

impl UltralyticsDetector {
    pub fn new(model_path: &str) -> anyhow::Result<Self> {
        info!("UltralyticsDetector - Loading model...");
        let model = ultralytics_inference::YOLOModel::load(model_path)?;

        Ok(Self { model })
    }
}

impl ObjectDetector for UltralyticsDetector {
    fn detect(
        &mut self,
        bytes: &[u8],
        min_confidence: f32,
        types: &[ObjectClass],
    ) -> anyhow::Result<Vec<Object>> {
        debug!("UltralyticsDetector - Loading image ...");
        let mut detections = Vec::new();

        let img = image::load_from_memory(bytes)?;
        let tmp = tempfile::Builder::new().suffix(".jpg").tempfile()?;
        img.save(tmp.path())?;

        let results = self.model.predict(tmp.path())?;

        for result in &results {
            if let Some(boxes) = &result.boxes {
                for i in 0..boxes.len() {
                    let confidence = boxes.conf()[i];
                    if confidence < min_confidence {
                        continue;
                    }

                    let class_id = boxes.cls()[i] as usize;
                    let class_name = result
                        .names
                        .get(&class_id)
                        .map_or("unknown", |s| s.as_str());

                    if !types.contains(&class_name.into()) {
                        continue;
                    }

                    let bbox = BoundingBox {
                        x1: boxes.xyxy()[[i, 0]],
                        y1: boxes.xyxy()[[i, 1]],
                        x2: boxes.xyxy()[[i, 2]],
                        y2: boxes.xyxy()[[i, 3]],
                    };

                    detections.push(Object {
                        class: class_name.into(),
                        confidence,
                        bbox,
                    });
                }
            }
        }

        Ok(detections)
    }
}
