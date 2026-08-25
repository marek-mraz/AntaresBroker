*** Settings ***
Documentation       Verify the 4.5.4 simplified (keyValues) representation per attribute type.
...
...                 Clause 4.5.4: "the value of the map item is the Property Value"
...                 (Property, Example 1) / "the Relationship's object" (Example 2) /
...                 the bare ordered array for ListProperty and ListRelationship
...                 (Examples 7-8), BUT a single-key object wrapping the subtype value
...                 for LanguageProperty {"languageMap": …} (Example 4), JsonProperty
...                 {"json": …} (Example 5) and VocabProperty {"vocab": …} (Example 6).
...                 Multi-instance attributes become {"dataset": {<datasetId>: …,
...                 "@none": …}} (Example 9).
...
...                 Antares extension TP — official keyValues TPs (018_10, 019_*)
...                 cover none of the wrapped subtypes nor the dataset map.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
454_01 Simplified Representation Per Attribute Type
    [Documentation]    4.5.4: bare forms for value/object/valueList/objectList; wrapped
    ...    single-key objects for languageMap/json/vocab; dataset map keyed by
    ...    datasetId with "@none" for the default instance. The simplified body must
    ...    NOT carry reified members (no "type"/"value" objects).
    [Tags]    e-retrieve    4_5_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": [{"type": "Property", "value": 55}, {"type": "Property", "value": 11, "datasetId": "urn:ngsi-ld:Dataset:gps"}], "brandName": {"type": "LanguageProperty", "languageMap": {"en": "hi"}}, "street": {"type": "JsonProperty", "json": {"a": 1}}, "category": {"type": "VocabProperty", "vocab": "non-commercial"}, "tyreTreadDepths": {"type": "ListProperty", "valueList": [1.2, 3.4]}, "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:1"}, "route": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Road:1", "urn:ngsi-ld:Road:2"]}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    options=keyValues
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${expected_lang}=    Evaluate    {"languageMap": {"en": "hi"}}
    Should Be Equal    ${body['brandName']}    ${expected_lang}
    ${expected_json}=    Evaluate    {"json": {"a": 1}}
    Should Be Equal    ${body['street']}    ${expected_json}
    ${expected_vocab}=    Evaluate    {"vocab": "non-commercial"}
    Should Be Equal    ${body['category']}    ${expected_vocab}
    ${expected_list}=    Evaluate    [1.2, 3.4]
    Should Be Equal    ${body['tyreTreadDepths']}    ${expected_list}
    Should Be Equal    ${body['isParked']}    urn:ngsi-ld:OffStreetParking:1
    ${expected_objlist}=    Evaluate    ["urn:ngsi-ld:Road:1", "urn:ngsi-ld:Road:2"]
    Should Be Equal    ${body['route']}    ${expected_objlist}
    ${expected_ds}=    Evaluate    {"dataset": {"@none": 55, "urn:ngsi-ld:Dataset:gps": 11}}
    Should Be Equal    ${body['speed']}    ${expected_ds}
    ${reified}=    Evaluate    [k for k, v in $body.items() if isinstance(v, dict) and "type" in v and "value" in v]
    Should Be Empty    ${reified}
