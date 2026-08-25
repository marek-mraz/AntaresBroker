*** Settings ***
Documentation       Verify 6.3.6 GeoJSON response @context placement.
...
...                 6.3.6: with Accept application/geo+json, if the Prefer
...                 header is omitted (or body=ld+json) the payload body
...                 shall include the JSON-LD @context; with
...                 "Prefer: body=json" the @context shall be OMITTED from
...                 the body and conveyed by the Link header only. Also: no
...                 Content-Length header on 204 responses.
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections
Library             RequestsLibrary

Test Setup          Create Fixture Entity
Test Teardown       Delete Fixture Entity


*** Variables ***
${entity_id}=       urn:ngsi-ld:Vehicle:geo636


*** Test Cases ***
636_01_01 GeoJSON Body Embeds The Context By Default
    [Documentation]    6.3.6: Prefer omitted → the geo+json body includes a
    ...    JSON-LD @context.
    [Tags]    common-behaviours    6_3_6    since_v1.9.1

    &{headers}=    Create Dictionary    Accept=application/geo+json
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=type=Vehicle
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.headers['Content-Type']}    application/geo+json
    Dictionary Should Contain Key    ${response.json()}    @context

636_01_02 Prefer Body Json Omits The Context From The Body
    [Documentation]    6.3.6: "Prefer: body=json" → @context omitted from the
    ...    geo+json body; the Link header carries it instead.
    [Tags]    common-behaviours    6_3_6    since_v1.9.1

    &{headers}=    Create Dictionary    Accept=application/geo+json    Prefer=body=json
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=type=Vehicle
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.headers['Content-Type']}    application/geo+json
    Dictionary Should Not Contain Key    ${response.json()}    @context
    Dictionary Should Contain Key    ${response.headers}    Link

636_01_03 No Content-Length On 204
    [Documentation]    6.3.6: "No Content-Length HTTP header shall be present
    ...    if the response code is 204."
    [Tags]    common-behaviours    6_3_6    since_v1.9.1

    ${response}=    DELETE
    ...    url=${url}/entities/${entity_id}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    Content-Length
    # recreate so the teardown delete still succeeds
    Create Fixture Entity


*** Keywords ***
Create Fixture Entity
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${body}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [8.5, 41.2]}}}
    ${response}=    POST
    ...    url=${url}/entities
    ...    json=${body}
    ...    headers=${headers}
    ...    expected_status=any
    Should Be True    ${response.status_code} in (201, 409)

Delete Fixture Entity
    ${response}=    DELETE
    ...    url=${url}/entities/${entity_id}
    ...    expected_status=any
