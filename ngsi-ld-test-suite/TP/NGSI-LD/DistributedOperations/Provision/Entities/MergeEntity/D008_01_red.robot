*** Settings ***
Documentation       Check that if one requests the Context Broker to merge an entity that matches a redirect registration, this is merged on the Context Sources.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Registration And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes-second.jsonld
${entity_new_payload_filename}          vehicle-simple-attributes-second-different.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld

*** Test Cases ***
D008_01_red Merge Entity On Both Context Broker And Context Source
    [Documentation]    Check that if one requests the Context Broker to merge an entity that matches a redirect registration, this is merged on the Context Source.
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_6_17

    Set Stub Reply    PATCH    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    Set Stub Reply    PATCH    /broker2/ngsi-ld/v1/entities/${entity_id}    204
    ${response}=    Merge Entity
    ...    entity_id=${entity_id}
    ...    entity_filename=${entity_new_payload_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    PATCH    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count}  > 0
    ${stub_count}=    Get Stub Count    PATCH    /broker2/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count}  > 0

*** Keywords ***
Setup Registration And Start Context Source Mock Server
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
