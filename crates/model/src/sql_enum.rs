/// Implements `sqlx::Type`/`Encode`/`Decode` for MySQL by round-tripping
/// through `&str`, matching a `VARCHAR ... CHECK (col IN (...))` column.
///
/// sqlx's `#[derive(sqlx::Type)]` for a plain (non-`#[repr]`) enum targets a
/// *native* MySQL `ENUM` column and compares the driver's exact reported
/// type metadata against a placeholder -- which fails even against a real
/// `ENUM(...)` column with the same member list, across at least sqlx
/// 0.8.x. Mapping through `&str`/`VARCHAR` instead is the well-established
/// workaround and also keeps these columns portable if this schema is ever
/// pointed at a different database.
macro_rules! sql_string_enum {
    ($name:ident { $($variant:ident => $str:literal),+ $(,)? }) => {
        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $str,)+
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($str => Ok(Self::$variant),)+
                    other => Err(format!(concat!("invalid ", stringify!($name), " value: '{}'"), other)),
                }
            }
        }

        impl sqlx::Type<sqlx::MySql> for $name {
            fn type_info() -> sqlx::mysql::MySqlTypeInfo {
                <&str as sqlx::Type<sqlx::MySql>>::type_info()
            }

            fn compatible(ty: &sqlx::mysql::MySqlTypeInfo) -> bool {
                <&str as sqlx::Type<sqlx::MySql>>::compatible(ty)
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::MySql> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut <sqlx::MySql as sqlx::Database>::ArgumentBuffer<'q>,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <&str as sqlx::Encode<'q, sqlx::MySql>>::encode(self.as_str(), buf)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::MySql> for $name {
            fn decode(
                value: <sqlx::MySql as sqlx::Database>::ValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let s = <&str as sqlx::Decode<'r, sqlx::MySql>>::decode(value)?;
                s.parse::<Self>().map_err(Into::into)
            }
        }
    };
}

pub(crate) use sql_string_enum;
