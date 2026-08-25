*** Settings ***
Documentation       Check the OrderingParams data type (CIM 009 clause 5.2.43,
...                 Table 5.2.43-1) on POST /entityOperations/query: orderBy String[]
...                 orders the payload, coordinates (a JSON array, mandatory when
...                 orderBy uses order by distance) + geometry form the distance
...                 reference, and an explicit collation is applied to string
...                 ordering (an unparseable collation tag is refused loudly).
...
...                 Antares extension TP — no official TP sends the ordering member.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Setup Ordering Entities
Suite Teardown      Delete Ordering Entities


*** Variables ***
${near_vehicle}=    urn:ngsi-ld:Vehicle:52430-near
${far_vehicle}=     urn:ngsi-ld:Vehicle:52430-far


*** Test Cases ***
5243_01_01 Ordering By Distance Ascending Orders Nearest First
    [Documentation]    Table 5.2.43-1: orderBy dist-asc with coordinates as the reference
    [Tags]    e-query    5_2_43    since_v1.9.1
    ${body}=    Post Ordering Query Expecting    200
    ...    {"orderBy": ["location;dist-asc"], "coordinates": [8, 40], "geometry": "Point"}
    ${near}=    Evaluate    $body.find("52430-near")
    ${far}=    Evaluate    $body.find("52430-far")
    Should Be True    ${near} < ${far}    dist-asc must order near before far

5243_01_02 Distance Ordering Without Coordinates Is Rejected
    [Documentation]    Table 5.2.43-1: coordinates "shall be one if orderBy uses order
    ...    by distance"
    [Tags]    e-query    5_2_43    since_v1.9.1
    Post Ordering Query Expecting    400    {"orderBy": ["location;dist-asc"]}

5243_01_03 Non-Array Coordinates Are Rejected
    [Tags]    e-query    5_2_43    since_v1.9.1
    Post Ordering Query Expecting    400    {"orderBy": ["location;dist-asc"], "coordinates": "8,40"}

5243_01_04 A Non-String Geometry Is Rejected
    [Tags]    e-query    5_2_43    since_v1.9.1
    Post Ordering Query Expecting    400    {"orderBy": ["id;asc"], "geometry": 5}

5243_01_05 An Explicit Collation Is Honoured
    [Documentation]    4.23.3/5.2.43: results "shall be ordered according to the
    ...    collation given" — German collation sorts "Ähre" before "Zebra", while
    ...    plain codepoint order would put "Zebra" (U+005A) before "Ähre" (U+00C4).
    [Tags]    e-query    5_2_43    since_v1.9.1
    ${body}=    Post Ordering Query Expecting    200
    ...    {"orderBy": ["name;asc"], "collation": "de-u-co-phonebk"}
    ${ahre}=    Evaluate    $body.find("52430-far")
    ${zebra}=    Evaluate    $body.find("52430-near")
    Should Be True    ${ahre} >= 0    the Ähre-named vehicle must be in the response
    Should Be True    ${zebra} >= 0    the Zebra-named vehicle must be in the response
    Should Be True    ${ahre} < ${zebra}    de collation must order Ähre before Zebra

5243_01_06 An Invalid Collation Tag Is Refused Loudly
    [Documentation]    4.23.3: a collation that cannot be honoured must be refused
    ...    (BadRequestData) — silent mis-ordering would violate "shall be ordered
    ...    according to the collation given".
    [Tags]    e-query    5_2_43    since_v1.9.1
    Post Ordering Query Expecting    400    {"orderBy": ["id;asc"], "collation": "not a bcp47 tag!!"}


*** Keywords ***
Post Ordering Query Expecting
    [Arguments]    ${expected_status_code}    ${ordering_json}
    ${payload}=    Evaluate
    ...    {"type": "Query", "entities": [{"type": "Vehicle"}], "ordering": json.loads('''${ordering_json}''')}
    ...    modules=json
    ${response}=    POST
    ...    url=${url}/${ENTITY_OPERATIONS_QUERY_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    RETURN    ${response.text}

Setup Ordering Entities
    FOR    ${id}    ${lon}    ${lat}    ${name}    IN
    ...    ${near_vehicle}    ${8.01}    ${40.01}    Zebra
    ...    ${far_vehicle}    ${10.0}    ${45.0}    Ähre
        ${payload}=    Evaluate
        ...    json.dumps({"id": $id, "type": "Vehicle", "name": {"type": "Property", "value": $name}, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [$lon, $lat]}}})
        ...    modules=json
        ${response}=    POST
        ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
        ...    data=${payload}
        ...    headers=${{ {"Content-Type": "application/json"} }}
        ...    expected_status=any
        Check Response Status Code    201    ${response.status_code}
    END

Delete Ordering Entities
    FOR    ${id}    IN    ${near_vehicle}    ${far_vehicle}
        ${response}=    DELETE
        ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${id}
        ...    expected_status=any
    END
