*** Settings ***
Documentation       Check that if one request the Context Broker to append an attribute whose id matches an inclusive registration, the update is performed on the Context Broker and forwarded to the Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${fragment_filename}                    vehicle-speed-isParked-fragment.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld

*** Test Cases ***
D003_01_inc Append Entity Attribute
    [Documentation]    Check that, given an inclusive registration, appending entity attributes updates the Context Source accordingly.
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_6_3

    ${response}=    Retrieve Entity    ${entity_id}
    ${old_body}=    Get From Dictionary    ${response.json()}    isParked

    Set Stub Reply    POST    /ngsi-ld/v1/entities/${entity_id}/attrs/    204
    ${response}=    Append Entity Attributes
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    POST    /ngsi-ld/v1/entities/${entity_id}/attrs/
    Should Be Equal As Integers    ${stub_count}    1

    ${response}=    Retrieve Entity    ${entity_id}
    ${new_body}=    Get From Dictionary    ${response.json()}    isParked

    Should Have Value In Json    ${response.json()}    $.speed
    Should Not Be Equal    ${old_body}    ${new_body}

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
