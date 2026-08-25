*** Settings ***
Documentation       Verify that, when one has a redirect registration on a Context Broker, one is able to replace the entities on the Context Sources

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Entity Id And Registration And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_replacement}                   vehicle-simple-attributes-second.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld


*** Test Cases ***
D007_01_red Replace Entity
    [Documentation]    Check that if one requests the Context Broker to replace an entity that matches redirect registrations, the entity is replaced on the Context Sources
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_6_18

    Set Stub Reply    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    Set Stub Reply    PUT    /broker2/ngsi-ld/v1/entities/${entity_id}    204

    ${response}=    Replace Entity
    ...    entity_id=${entity_id}
    ...    filename=${entity_replacement}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be Equal As Integers    ${stub_count}    1
    ${stub_count}=    Get Stub Count    PUT    /broker2/ngsi-ld/v1/entities/${entity_id}
    Should Be Equal As Integers    ${stub_count}    1


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

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Stop Context Source Mock Server
