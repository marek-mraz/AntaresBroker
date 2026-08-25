*** Settings ***
Documentation       Verify the 4.5.2.3 concise Property representation on input.
...
...                 Clause 4.5.2.3: a Property without sub-attributes "shall be
...                 represented in a concise but lossless representation by a member
...                 whose key is the Property name and whose value is the Property
...                 Value"; with sub-attributes, an object with mandatory "value" and
...                 no "type" ("Property can be inferred by the presence of the value
...                 attribute"); "the GeoProperty sub-type shall be inferred instead,
...                 if the Property Value resolves to a supported GeoJSON geometry."
...
...                 Antares extension TP — the suite contains zero concise-input
...                 coverage.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
452_02 Concise Creation Round-Trips To Normalized Types
    [Documentation]    4.5.2.3: bare value ⇒ Property; geometry value ⇒ GeoProperty
    ...    (whole-object and value-member forms); type-less object with value and a
    ...    sub-attribute ⇒ Property with the sub-attribute preserved.
    [Tags]    e-create    e-retrieve    4_5_2_3    4_5_3_3    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": 55, "location": {"type": "Point", "coordinates": [8.0, 49.0]}, "area": {"value": {"type": "Point", "coordinates": [1.0, 2.0]}}, "brandName": {"value": "Volvo", "observedAt": "2026-01-01T00:00:00Z"}, "isParked": {"object": "urn:ngsi-ld:OffStreetParking:1"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${speed_type}=    Evaluate    $body['speed']['type']
    Should Be Equal    ${speed_type}    Property
    ${speed_value}=    Evaluate    $body['speed']['value']
    Should Be Equal As Integers    ${speed_value}    55
    ${loc_type}=    Evaluate    $body['location']['type']
    Should Be Equal    ${loc_type}    GeoProperty
    ${area_type}=    Evaluate    $body['area']['type']
    Should Be Equal    ${area_type}    GeoProperty
    ${brand_type}=    Evaluate    $body['brandName']['type']
    Should Be Equal    ${brand_type}    Property
    ${brand_obs}=    Evaluate    $body['brandName']['observedAt']
    Should Be Equal    ${brand_obs}    2026-01-01T00:00:00Z
    ${rel_type}=    Evaluate    $body['isParked']['type']
    Should Be Equal    ${rel_type}    Relationship
