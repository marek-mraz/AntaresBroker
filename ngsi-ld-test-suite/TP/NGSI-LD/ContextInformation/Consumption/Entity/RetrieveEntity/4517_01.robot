*** Settings ***
Documentation       Verify the 4.5.17 simplified GeoJSON representation.
...
...                 4.5.17.0: "When both simplified (see clause 4.5.4) and GeoJSON
...                 representation is requested" the Feature's properties carry the
...                 simplified attribute values; multi-attribute Properties become
...                 the {"dataset": {<datasetId>|"@none": …}} map (4.5.17.1) and
...                 geometry selection follows 4.5.16.1 (the default instance).
...
...                 Antares extension TP — no official TP combines
...                 options=keyValues with Accept: application/geo+json.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
4517_01 Simplified Feature With Dataset Map And Default Geometry
    [Documentation]    4.5.17.1: properties are simplified values (bare Property
    ...    value, dataset map for multi-instance); geometry = the default
    ...    instance's geometry; fixed type "Feature".
    [Tags]    e-retrieve    4_5_17_1    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [9.0, 9.0]}, "datasetId": "urn:ngsi-ld:Dataset:gps"}, {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [3.0, 4.0]}}], "speed": {"type": "Property", "value": 5}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=application/geo+json
    ...    options=keyValues
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${ftype}=    Evaluate    $body['type']
    Should Be Equal    ${ftype}    Feature
    ${geom}=    Evaluate    $body['geometry']
    ${expected_geom}=    Evaluate    {"type": "Point", "coordinates": [3.0, 4.0]}
    Should Be Equal    ${geom}    ${expected_geom}
    ${speed}=    Evaluate    $body['properties']['speed']
    Should Be Equal As Integers    ${speed}    5
    ${loc}=    Evaluate    $body['properties']['location']
    ${loc_keys}=    Evaluate    sorted($loc.keys())
    ${expected_keys}=    Evaluate    ['dataset']
    Should Be Equal    ${loc_keys}    ${expected_keys}
    ${ds_keys}=    Evaluate    sorted($loc['dataset'].keys())
    ${expected_ds}=    Evaluate    ['@none', 'urn:ngsi-ld:Dataset:gps']
    Should Be Equal    ${ds_keys}    ${expected_ds}
    ${reified}=    Evaluate    [k for k, v in $body['properties'].items() if isinstance(v, dict) and 'type' in v and 'value' in v]
    Should Be Empty    ${reified}
