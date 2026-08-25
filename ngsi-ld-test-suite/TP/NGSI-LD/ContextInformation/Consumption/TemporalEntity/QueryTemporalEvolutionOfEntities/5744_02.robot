*** Settings ***
Documentation       Verify 4.9 operator/datatype edges on the 5.7.4.4 values
...                 filter, second batch: p.91 "no equality between the
...                 target value data type and the Query Term value data
...                 type → not matching" (and its != inverse), p.89
...                 "Relationship ... with any operator different than equal
...                 or unequal shall result in not matching", p.92 pattern
...                 on non-String never matches, p.90-91 languageMap [lang]
...                 and [*] filters, ValueList against array targets,
...                 unequal-range, temporal ordering of date values, boolean
...                 equality, parenthesized grouping, and 5.7.4.4 S3: the
...                 geoquery judged only on GeoProperty instances within the
...                 temporal interval. Antares extension TP.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Fixture Entities
Suite Teardown      Delete Fixture Entities


*** Variables ***
${typed}=       urn:ngsi-ld:Vehicle:qp5744b-typed
${typed2}=      urn:ngsi-ld:Vehicle:qp5744b-second
${langv}=       urn:ngsi-ld:Vehicle:qp5744b-langv
${langv2}=      urn:ngsi-ld:Vehicle:qp5744b-langother
${relv}=        urn:ngsi-ld:Vehicle:qp5744b-relv
${arrv}=        urn:ngsi-ld:Vehicle:qp5744b-arrv
${fastest}=     urn:ngsi-ld:Vehicle:qp5744b-fastest
${geoin}=       urn:ngsi-ld:Vehicle:qp5744b-geoin
${geoout}=      urn:ngsi-ld:Vehicle:qp5744b-geoout
${IN}=          2026-03-01T12:00:00Z
${OUT}=         2026-01-05T00:00:00Z
${window}=      timerel=between&timeAt=2026-03-01T00:00:00Z&endTimeAt=2026-03-02T00:00:00Z


*** Test Cases ***
5744_02_01 Equality Requires Matching Datatypes
    [Documentation]    4.9 p.91: string "30" vs number 30 — no datatype
    ...    equality means not matching; the numeric literal matches.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3D%3D%2230%22
    Should Not Contain    ${response.text}    ${typed}
    ${response}=    Query Temporal    speed%3D%3D30
    Should Contain    ${response.text}    ${typed}

5744_02_02 Datatype Mismatch Satisfies Unequal
    [Documentation]    4.9 p.92: "If the data type of the target value and
    ...    the data type of the Query Term value are different, then they
    ...    shall be considered unequal" — number 30 != string "thirty".
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%21%3D%22thirty%22
    Should Contain    ${response.text}    ${typed}
    Should Not Contain    ${response.text}    ${typed2}

5744_02_03 Ordering Requires Matching Datatypes
    [Documentation]    4.9 p.92 Greater than: string "550" vs number 100 is
    ...    not matching; against a string literal the order holds.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    route%3E100
    Should Not Contain    ${response.text}    ${typed}
    ${response}=    Query Temporal    route%3E%22200%22
    Should Contain    ${response.text}    ${typed}

5744_02_04 Relationship With Ordering Operator Never Matches
    [Documentation]    4.9 p.89: "If the target element corresponds to a
    ...    Relationship ... any operator different than equal or unequal
    ...    shall result in not matching."
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    ref%3E%22urn:a%22
    Should Not Contain    ${response.text}    ${relv}
    ${response}=    Query Temporal    ref%3D%3D%22urn:dest:qpA%22
    Should Contain    ${response.text}    ${relv}

5744_02_05 Pattern On A Non-String Never Matches
    [Documentation]    4.9 p.92: "If the target value data type is
    ...    different than String then it shall be considered as not
    ...    matching" — a numeric speed never matches ~=.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed~%3D%223.*%22
    Should Not Contain    ${response.text}    ${typed}
    ${response}=    Query Temporal    route~%3D%225.*%22
    Should Contain    ${response.text}    ${typed}

5744_02_06 Do-Not-Match Pattern
    [Documentation]    4.9 p.92 notPatternOp: the target value shall NOT be
    ...    in L(R) — "550" escapes 9.* but not 5.*.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    route%21~%3D%229.*%22
    Should Contain    ${response.text}    ${typed}
    ${response}=    Query Temporal    route%21~%3D%225.*%22
    Should Not Contain    ${response.text}    ${typed}

5744_02_07 Language Filter Selects One Language Key
    [Documentation]    4.9 p.90: label[en]=="red" matches only via the "en"
    ...    key of the languageMap; the same value under another key or a
    ...    different language filter must not match.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    label%5Ben%5D%3D%3D%22red%22
    Should Contain    ${response.text}    ${langv}
    Should Not Contain    ${response.text}    ${langv2}
    ${response}=    Query Temporal    label%5Bfr%5D%3D%3D%22red%22
    Should Not Contain    ${response.text}    ${langv}

5744_02_08 Language Wildcard Matches Any Language
    [Documentation]    4.9 p.90: label[*]=="rouge" — any languageMap value
    ...    matches.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    label%5B*%5D%3D%3D%22rouge%22
    Should Contain    ${response.text}    ${langv}
    Should Contain    ${response.text}    ${langv2}

5744_02_09 ValueList Against An Array Target
    [Documentation]    4.9 p.90: "The target value includes any of the
    ...    Query Term values, if the target value is an array" — ["a","b"]
    ...    matches "b","z" but not "x","z".
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    tags%3D%3D%22b%22,%22z%22
    Should Contain    ${response.text}    ${arrv}
    ${response}=    Query Temporal    tags%3D%3D%22x%22,%22z%22
    Should Not Contain    ${response.text}    ${arrv}

5744_02_10 Unequal Against A Range
    [Documentation]    4.9 p.91: != range matches only values outside the
    ...    closed interval — 99 escapes 10..40, 30 does not.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%21%3D10..40
    Should Contain    ${response.text}    ${fastest}
    Should Not Contain    ${response.text}    ${typed}

5744_02_11 Date Values Order Temporally
    [Documentation]    4.9 p.89: "When comparing dates or times, the order
    ...    relation considered shall be a temporal one."
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    departure%3E%222026-02-15T00:00:00Z%22
    Should Contain    ${response.text}    ${typed}
    Should Not Contain    ${response.text}    ${typed2}

5744_02_12 Boolean Equality
    [Documentation]    4.9: boolean literals match only their own value.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    active%3D%3Dtrue
    Should Contain    ${response.text}    ${typed}
    Should Not Contain    ${response.text}    ${typed2}
    ${response}=    Query Temporal    active%3D%3Dfalse
    Should Contain    ${response.text}    ${typed2}
    Should Not Contain    ${response.text}    ${typed}

5744_02_13 Parenthesized Grouping Gates The Conjunction
    [Documentation]    4.9: (speed>25|route=="551");active==true — an
    ...    entity satisfying only the disjunction (fastest, no active)
    ...    must not pass the conjunction.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    %28speed%3E25%7Croute%3D%3D%22551%22%29%3Bactive%3D%3Dtrue
    Should Contain    ${response.text}    ${typed}
    Should Not Contain    ${response.text}    ${fastest}

5744_02_14 Geoquery Judged Only On Windowed Instances
    [Documentation]    5.7.4.4 S3: "those geospatial restrictions shall be
    ...    checked against the GeoProperty instances that are within the
    ...    interval defined by the temporal query" — an entity whose only
    ...    near position is out-of-window must not match.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal Raw    georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B17.1,48.1%5D
    Should Contain    ${response.text}    ${geoin}
    Should Not Contain    ${response.text}    ${geoout}


*** Keywords ***
Query Temporal
    [Arguments]    ${q_expr}
    ${response}=    Query Temporal Raw    q=${q_expr}
    RETURN    ${response}

Query Temporal Raw
    [Arguments]    ${qs}
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&${qs}&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    RETURN    ${response}

Create Temporal Entity
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Create Fixture Entities
    Create Temporal Entity    {"id": "${typed}", "type": "Vehicle", "speed": [{"type": "Property", "value": 30, "observedAt": "${IN}"}], "route": [{"type": "Property", "value": "550", "observedAt": "${IN}"}], "active": [{"type": "Property", "value": true, "observedAt": "${IN}"}], "departure": [{"type": "Property", "value": "2026-03-01T06:00:00Z", "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${typed2}", "type": "Vehicle", "active": [{"type": "Property", "value": false, "observedAt": "${IN}"}], "departure": [{"type": "Property", "value": "2026-02-01T00:00:00Z", "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${langv}", "type": "Vehicle", "label": [{"type": "LanguageProperty", "languageMap": {"en": "red", "fr": "rouge"}, "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${langv2}", "type": "Vehicle", "label": [{"type": "LanguageProperty", "languageMap": {"en": "black", "fr": "rouge"}, "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${relv}", "type": "Vehicle", "ref": [{"type": "Relationship", "object": "urn:dest:qpA", "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${arrv}", "type": "Vehicle", "tags": [{"type": "Property", "value": ["a", "b"], "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${fastest}", "type": "Vehicle", "speed": [{"type": "Property", "value": 99, "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${geoin}", "type": "Vehicle", "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [17.1001, 48.1001]}, "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${geoout}", "type": "Vehicle", "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [10.0, 40.0]}, "observedAt": "${IN}"}, {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [17.1, 48.1]}, "observedAt": "${OUT}"}]}

Delete Fixture Entities
    FOR    ${id}    IN    ${typed}    ${typed2}    ${langv}    ${langv2}    ${relv}    ${arrv}    ${fastest}    ${geoin}    ${geoout}
        ${response}=    DELETE    url=${temporal_api_url}/temporal/entities/${id}    expected_status=any
    END
