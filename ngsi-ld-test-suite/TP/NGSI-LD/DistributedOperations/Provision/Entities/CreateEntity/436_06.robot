*** Settings ***
Documentation       Verify the 4.3.6.6 processed contextSourceInfo keys on the wire.
...
...                 Clause 4.3.6.6: "contentType" — "the Context Broker shall provide
...                 the request and the associated @context as required by the MIME
...                 type when distributing the request"; "jsonldContext" — compaction
...                 with the registered context, "the Content-Type of the forwarded
...                 request shall be application/json and the Context Broker shall
...                 remove any @context members from the payload"; "accept" — the
...                 response from the distributed endpoint shall be returned in the
...                 defined format (the forward asks for it via the Accept header).
...
...                 Antares extension TP — no official TP exercises the 4.3.6.6
...                 processed keys.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_speed_filename}                vehicle-speed-attribute.jsonld
${stub_entity_filename}                 vehicle-speed-attribute.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld
${attribute_fragment_filename}          vehicle-speed-no-datasetid-fragment.jsonld


*** Test Cases ***
436_06_01 Registered contentType ld+json Forwards An Inline Context
    [Documentation]    4.3.6.6 contentType: the broker shall provide the request and
    ...    the associated @context as the MIME type requires — application/ld+json
    ...    means Content-Type ld+json with the @context inline in the payload.
    [Tags]    dist-ops    4_3_6_6    5_6_1    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "contentType", "value": "application/ld+json"}]
    Setup Registration With ContextSourceInfo And Start Mock    ${csi}
    Set Stub Reply    POST    /ngsi-ld/v1/entities    201
    ${response}=    Create Entity    ${entity_speed_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    Wait For Request
    ${headers}=    Get Request Headers
    ${ct}=    Evaluate    str($headers.get('Content-Type'))
    Should Contain    ${ct}    application/ld+json
    ${body}=    Get Request Body
    ${parsed}=    Evaluate    json.loads($body)    json
    Dictionary Should Contain Key    ${parsed}    @context

436_06_02 Registered jsonldContext Forwards Recompacted JSON Without Context
    [Documentation]    4.3.6.6 jsonldContext: the broker shall recompact payload and
    ...    query parameters with the registered context, forward with Content-Type
    ...    application/json and remove any @context members from the payload.
    [Tags]    dist-ops    4_3_6_6    5_6_1    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "jsonldContext", "value": $ngsild_test_suite_context}]
    Setup Registration With ContextSourceInfo And Start Mock    ${csi}
    Set Stub Reply    POST    /ngsi-ld/v1/entities    201
    ${response}=    Create Entity    ${entity_speed_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    Wait For Request
    ${headers}=    Get Request Headers
    ${ct}=    Evaluate    str($headers.get('Content-Type'))
    Should Contain    ${ct}    application/json
    Should Not Contain    ${ct}    ld+json
    ${link}=    Evaluate    str($headers.get('Link'))
    Should Contain    ${link}    ${ngsild_test_suite_context}
    ${body}=    Get Request Body
    ${parsed}=    Evaluate    json.loads($body)    json
    Dictionary Should Not Contain Key    ${parsed}    @context

436_06_03 Registered accept Steers The Forwarded Accept Header
    [Documentation]    4.3.6.6 accept: the response from the distributed endpoint
    ...    shall be returned in the registered format — the forwarded read asks for
    ...    it with an Accept header carrying the registered MIME type.
    [Tags]    dist-ops    4_3_6_6    5_7_1    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "accept", "value": "application/ld+json"}]
    Setup Registration With ContextSourceInfo And Start Mock    ${csi}
    ${entity_body}=    Load Entity    ${stub_entity_filename}    ${entity_id}
    ${entity_body_json}=    Evaluate    json.dumps($entity_body)    json
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    200    ${entity_body_json}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request
    ${headers}=    Get Request Headers
    ${accept}=    Evaluate    str($headers.get('Accept'))
    Should Contain    ${accept}    application/ld+json


436_06_04 Registered jsonldContext Leaves An Attribute Fragment A Fragment
    [Documentation]    4.3.6.6 jsonldContext: the compaction applies "over both
    ...    payload and query parameters". A 5.6.4 Attribute Fragment is not an
    ...    Entity — its "type" is an Attribute type (4.5.2) and its "value" is the
    ...    Property value — so the Context Source shall receive the fragment the
    ...    client sent. Read as an Entity, "value" becomes a sub-Attribute name and
    ...    the value the Context Source stores is not the one that was written.
    [Tags]    dist-ops    4_3_6_6    5_6_4    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "jsonldContext", "value": $ngsild_test_suite_context}]
    Setup Registration With ContextSourceInfo And Start Mock    ${csi}
    Set Stub Reply    PATCH    /ngsi-ld/v1/entities/${entity_id}/attrs/speed    204
    ${response}=    Partial Update Entity Attributes
    ...    ${entity_id}
    ...    speed
    ...    ${attribute_fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}
    Wait For Request
    ${body}=    Get Request Body
    ${parsed}=    Evaluate    json.loads($body)    json
    Should Be Equal As Integers    ${parsed}[value]    56
    ${sub}=    Set Variable    ${parsed}[source]
    Should Be Equal As Strings    ${sub}[value]    Speedometer

*** Keywords ***
Setup Registration With ContextSourceInfo And Start Mock
    [Arguments]    ${csi}
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ${csi_dict}=    Create Dictionary    contextSourceInfo=${csi}
    ${registration_payload}=    Add Object To JSON
    ...    ${registration_payload}
    ...    $
    ...    ${csi_dict}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
