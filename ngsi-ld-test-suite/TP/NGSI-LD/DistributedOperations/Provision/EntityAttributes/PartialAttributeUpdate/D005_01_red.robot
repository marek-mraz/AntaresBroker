*** Settings ***
Documentation       Check that, given a redirect registration, partially updating an entity updates the Context Source accordingly.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Entity Id And Registration And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-different-attributes.jsonld
${fragment_filename}                    vehicle-speed-two-datasetid-01-fragment.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld
${attribute_id}                         speed


*** Test Cases ***
D005_01_red Partial Partial Attribute Update
    [Documentation]    Check that if one request the Context Broker to partially update an attribute whose id matches a redirect registration, the entity is updated on the Context Source
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_6_4

    Set Stub Reply    PATCH    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/${attribute_id}    204
    Set Stub Reply    PATCH    /broker2/ngsi-ld/v1/entities/${entity_id}/attrs/${attribute_id}    204
    ${response}=    Partial Update Entity Attributes
    ...    ${entity_id}
    ...    ${attribute_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Wait For Request
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    PATCH    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/${attribute_id}
    Should Be True    ${stub_count} > 0
    ${request_payload}=    Get Request Body
    ${payload}=    Evaluate    json.loads('''${request_payload}''')    json
    Should Contain    ${payload}    speed

    ${stub_count}=    Get Stub Count    PATCH    /broker2/ngsi-ld/v1/entities/${entity_id}/attrs/${attribute_id}
    Should Be True    ${stub_count} > 0
    ${request_payload}=    Get Request Body
    ${payload}=    Evaluate    json.loads('''${request_payload}''')    json
    Should Contain    ${payload}    speed


*** Keywords ***
Setup Entity Id And Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
